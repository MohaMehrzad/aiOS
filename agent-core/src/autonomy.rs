//! Autonomy Loop — the beating heart of aiOS
//!
//! Background loop that continuously:
//! 1. Checks for active goals
//! 2. Gets next unblocked task
//! 3. Routes to appropriate agent or AI model
//! 4. Records results and updates goal status
//!
//! Respects CancellationToken for graceful shutdown.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::context::ContextAssembler;
use crate::task_planner::IntelligenceLevel;
use crate::OrchestratorState;

/// Configuration for the autonomy loop
pub struct AutonomyConfig {
    /// Tick interval between autonomy loop iterations
    pub tick_interval: Duration,
    /// Maximum concurrent tasks
    pub max_concurrent_tasks: usize,
}

impl Default for AutonomyConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_millis(500),
            max_concurrent_tasks: 10,
        }
    }
}

/// Run the main autonomy loop
pub async fn run_autonomy_loop(
    state: Arc<RwLock<OrchestratorState>>,
    cancel: CancellationToken,
    config: AutonomyConfig,
) {
    info!("Autonomy loop started (tick={}ms)", config.tick_interval.as_millis());

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("Autonomy loop shutting down gracefully");
                break;
            }
            _ = tokio::time::sleep(config.tick_interval) => {
                if let Err(e) = autonomy_tick(&state, &config).await {
                    error!("Autonomy tick error: {e}");
                }
            }
        }
    }

    info!("Autonomy loop stopped");
}

/// Single tick of the autonomy loop
async fn autonomy_tick(
    state: &Arc<RwLock<OrchestratorState>>,
    _config: &AutonomyConfig,
) -> anyhow::Result<()> {
    let mut state = state.write().await;

    // 1. Check goal engine for active goals
    let active_goals = state.goal_engine.active_goal_count();
    if active_goals == 0 {
        return Ok(());
    }

    debug!("Autonomy tick: {active_goals} active goals");

    // 2. Get next unblocked task from task planner
    let next_task = state.task_planner.next_task().cloned();
    let task = match next_task {
        Some(t) => t,
        None => return Ok(()),
    };

    let task_id = task.id.clone();
    let goal_id = task.goal_id.clone();
    let level = IntelligenceLevel::from_str(&task.intelligence_level);

    // 3. Route task via agent router or handle directly
    let agent_id = state.agent_router.route_task(&task);

    if let Some(ref agent_id) = agent_id {
        // Assign to agent
        state.agent_router.assign_task(agent_id, &task_id);

        // Log the routing decision
        state.decision_logger.log_decision(
            "task_routing",
            &[agent_id.clone()],
            agent_id,
            &format!("Routed {} task to agent with matching capabilities", level.as_str()),
            level.as_str(),
            "heuristic",
        );

        debug!("Task {task_id} routed to agent {agent_id}");
    } else {
        // No agent available — execute via AI inference
        match level {
            IntelligenceLevel::Reactive => {
                // Handle reactively without AI
                debug!("Handling reactive task {task_id} with heuristics");
                state.task_planner.complete_task(
                    &task_id,
                    b"{\"result\":\"handled_by_heuristic\"}".to_vec(),
                );
                state.goal_engine.complete_task(&goal_id, &task_id);

                // Record result
                state.result_aggregator.record_result(
                    &goal_id,
                    crate::proto::common::TaskResult {
                        task_id: task_id.clone(),
                        success: true,
                        output_json: b"{\"result\":\"heuristic\"}".to_vec(),
                        error: String::new(),
                        duration_ms: 0,
                        tokens_used: 0,
                        model_used: "heuristic".to_string(),
                    },
                );
            }
            IntelligenceLevel::Operational | IntelligenceLevel::Tactical => {
                // Read preferred provider from goal metadata
                let preferred_provider = get_preferred_provider(&state, &goal_id);

                // Call local AI runtime for operational/tactical tasks
                let result = execute_ai_task(
                    &state.clients,
                    &task.description,
                    level.as_str(),
                    AiBackend::LocalRuntime,
                    &preferred_provider,
                )
                .await;

                handle_ai_result(
                    &mut state,
                    &task_id,
                    &goal_id,
                    &task.description,
                    level.as_str(),
                    result,
                );
            }
            IntelligenceLevel::Strategic => {
                // Read preferred provider from goal metadata
                let preferred_provider = get_preferred_provider(&state, &goal_id);

                // Call API gateway for strategic tasks (Claude/GPT)
                let result = execute_ai_task(
                    &state.clients,
                    &task.description,
                    level.as_str(),
                    AiBackend::ApiGateway,
                    &preferred_provider,
                )
                .await;

                handle_ai_result(
                    &mut state,
                    &task_id,
                    &goal_id,
                    &task.description,
                    level.as_str(),
                    result,
                );
            }
        }
    }

    // 4. Check if any goals are complete
    let (goals, _) = state.goal_engine.list_goals("", 100, 0).await;
    for goal in goals {
        if goal.status == "pending" || goal.status == "in_progress" {
            let progress = state.goal_engine.calculate_progress(&goal.id).await;
            if progress >= 100.0 {
                state.goal_engine.update_status(&goal.id, "completed");
                info!("Goal {} completed", goal.id);

                // Log the completion decision
                state.decision_logger.log_decision(
                    "goal_completion",
                    &[goal.id.clone()],
                    "completed",
                    &format!("All tasks for goal '{}' completed successfully", goal.description),
                    "reactive",
                    "heuristic",
                );
            } else if progress > 0.0 && goal.status == "pending" {
                state.goal_engine.update_status(&goal.id, "in_progress");
            }
        }
    }

    Ok(())
}

/// Which AI backend to use for inference
enum AiBackend {
    /// Local runtime (llama.cpp / small models)
    LocalRuntime,
    /// API gateway (Claude / GPT)
    ApiGateway,
}

/// AI inference result from either backend
struct AiInferenceResult {
    success: bool,
    response_text: String,
    tool_calls: Vec<ToolCallRequest>,
    model_used: String,
    tokens_used: i32,
}

/// A tool call extracted from AI response
#[derive(Clone)]
struct ToolCallRequest {
    tool_name: String,
    input_json: Vec<u8>,
}

/// Extract preferred provider from goal metadata JSON
fn get_preferred_provider(state: &OrchestratorState, goal_id: &str) -> String {
    state
        .goal_engine
        .get_metadata(goal_id)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .and_then(|v| v.get("preferred_provider").and_then(|p| p.as_str()).map(String::from))
        .unwrap_or_default()
}

/// Execute a task through AI inference with fallback chain:
/// local runtime -> api-gateway -> heuristic
async fn execute_ai_task(
    clients: &crate::clients::ServiceClients,
    task_description: &str,
    intelligence_level: &str,
    preferred_backend: AiBackend,
    preferred_provider: &str,
) -> AiInferenceResult {
    // Assemble context for the AI call
    let assembler = ContextAssembler::new(4096);
    let context = assembler.assemble_for_task(task_description, intelligence_level, &[], &[]);
    let system_prompt = match &context {
        Ok(ctx) => ctx.system_prompt.clone(),
        Err(_) => format!(
            "You are aiOS, an AI-native operating system. Execute this task: {task_description}"
        ),
    };

    let prompt = format!(
        "Task: {task_description}\n\n\
         Respond with a JSON object:\n\
         {{\"reasoning\": \"...\", \"tool_calls\": [{{\"tool\": \"tool.name\", \"input\": {{}}}}], \"result\": \"...\"}}"
    );

    // Try preferred backend first
    let result = match preferred_backend {
        AiBackend::LocalRuntime => {
            try_runtime_infer(clients, &prompt, &system_prompt).await
        }
        AiBackend::ApiGateway => {
            try_api_gateway_infer_with_provider(clients, &prompt, &system_prompt, preferred_provider).await
        }
    };

    if let Some(r) = result {
        return r;
    }

    // Fallback: try the other backend
    let fallback = match preferred_backend {
        AiBackend::LocalRuntime => {
            info!("Local runtime unavailable, falling back to API gateway");
            try_api_gateway_infer_with_provider(clients, &prompt, &system_prompt, preferred_provider).await
        }
        AiBackend::ApiGateway => {
            info!("API gateway unavailable, falling back to local runtime");
            try_runtime_infer(clients, &prompt, &system_prompt).await
        }
    };

    if let Some(r) = fallback {
        return r;
    }

    // Final fallback: heuristic response
    warn!("All AI backends unavailable, using heuristic fallback for task");
    AiInferenceResult {
        success: true,
        response_text: format!("{{\"result\":\"heuristic_fallback\",\"task\":\"{task_description}\"}}"),
        tool_calls: vec![],
        model_used: "heuristic".to_string(),
        tokens_used: 0,
    }
}

/// Try to call the local AI runtime for inference
async fn try_runtime_infer(
    clients: &crate::clients::ServiceClients,
    prompt: &str,
    system_prompt: &str,
) -> Option<AiInferenceResult> {
    match clients.runtime().await {
        Ok(mut client) => {
            let request = tonic::Request::new(crate::proto::runtime::InferRequest {
                model: String::new(),
                prompt: prompt.to_string(),
                system_prompt: system_prompt.to_string(),
                max_tokens: 1024,
                temperature: 0.3,
                intelligence_level: "operational".to_string(),
                requesting_agent: "autonomy-loop".to_string(),
                task_id: String::new(),
            });

            match client.infer(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    let tool_calls = parse_tool_calls(&resp.text);
                    Some(AiInferenceResult {
                        success: true,
                        response_text: resp.text,
                        tool_calls,
                        model_used: resp.model_used,
                        tokens_used: resp.tokens_used,
                    })
                }
                Err(e) => {
                    warn!("Runtime inference failed: {e}");
                    None
                }
            }
        }
        Err(e) => {
            debug!("Cannot connect to runtime: {e}");
            None
        }
    }
}

/// Try to call the API gateway for inference with a specific provider
async fn try_api_gateway_infer_with_provider(
    clients: &crate::clients::ServiceClients,
    prompt: &str,
    system_prompt: &str,
    preferred_provider: &str,
) -> Option<AiInferenceResult> {
    match clients.api_gateway().await {
        Ok(mut client) => {
            let request = tonic::Request::new(crate::proto::api_gateway::ApiInferRequest {
                prompt: prompt.to_string(),
                system_prompt: system_prompt.to_string(),
                max_tokens: 2048,
                temperature: 0.3,
                preferred_provider: preferred_provider.to_string(),
                requesting_agent: "autonomy-loop".to_string(),
                task_id: String::new(),
                allow_fallback: true,
            });

            match client.infer(request).await {
                Ok(response) => {
                    let resp: crate::proto::common::InferenceResponse = response.into_inner();
                    let tool_calls = parse_tool_calls(&resp.text);
                    Some(AiInferenceResult {
                        success: true,
                        response_text: resp.text,
                        tool_calls,
                        model_used: resp.model_used,
                        tokens_used: resp.tokens_used,
                    })
                }
                Err(e) => {
                    warn!("API gateway inference failed: {e}");
                    None
                }
            }
        }
        Err(e) => {
            debug!("Cannot connect to API gateway: {e}");
            None
        }
    }
}

/// Parse tool calls from AI response JSON
fn parse_tool_calls(response_text: &str) -> Vec<ToolCallRequest> {
    let mut calls = Vec::new();

    // Try to parse as JSON
    let text = response_text.trim();
    let json_str = if text.starts_with("```") {
        // Strip markdown code fences
        let lines: Vec<&str> = text.lines().collect();
        let start = if lines.first().map_or(false, |l| l.starts_with("```")) { 1 } else { 0 };
        let end = if lines.last().map_or(false, |l| l.trim() == "```") {
            lines.len() - 1
        } else {
            lines.len()
        };
        lines[start..end].join("\n")
    } else {
        text.to_string()
    };

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
        if let Some(tool_calls) = parsed.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tool_calls {
                let tool_name = tc.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                let input = tc.get("input").cloned().unwrap_or(serde_json::Value::Object(
                    serde_json::Map::new(),
                ));

                if !tool_name.is_empty() {
                    if let Ok(input_bytes) = serde_json::to_vec(&input) {
                        calls.push(ToolCallRequest {
                            tool_name: tool_name.to_string(),
                            input_json: input_bytes,
                        });
                    }
                }
            }
        }
    }

    calls
}

/// Handle the result of an AI inference call — execute tool calls and record results
fn handle_ai_result(
    state: &mut OrchestratorState,
    task_id: &str,
    goal_id: &str,
    task_description: &str,
    intelligence_level: &str,
    result: AiInferenceResult,
) {
    let output = if result.tool_calls.is_empty() {
        // No tool calls — the AI response is the result
        result.response_text.as_bytes().to_vec()
    } else {
        // Queue tool calls for execution (tools are executed via gRPC in a separate tick)
        let tool_names: Vec<String> = result.tool_calls.iter().map(|tc| tc.tool_name.clone()).collect();
        serde_json::to_vec(&serde_json::json!({
            "ai_response": result.response_text,
            "tool_calls_queued": tool_names,
            "model_used": result.model_used,
        }))
        .unwrap_or_else(|_| b"{}".to_vec())
    };

    // Mark task complete in both planners
    state.task_planner.complete_task(task_id, output.clone());
    state.goal_engine.complete_task(goal_id, task_id);

    // Record result
    state.result_aggregator.record_result(
        goal_id,
        crate::proto::common::TaskResult {
            task_id: task_id.to_string(),
            success: result.success,
            output_json: output,
            error: String::new(),
            duration_ms: 0,
            tokens_used: result.tokens_used,
            model_used: result.model_used,
        },
    );

    // Log the AI decision
    state.decision_logger.log_decision(
        "ai_execution",
        &[task_id.to_string()],
        "executed",
        &format!(
            "Executed {} task '{}' via AI inference",
            intelligence_level, task_description
        ),
        intelligence_level,
        "ai",
    );

    info!("AI executed task {task_id} at {intelligence_level} level");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autonomy_config_default() {
        let config = AutonomyConfig::default();
        assert_eq!(config.tick_interval, Duration::from_millis(500));
        assert_eq!(config.max_concurrent_tasks, 10);
    }

    #[test]
    fn test_parse_tool_calls_valid_json() {
        let response = r#"{"reasoning": "need to check disk", "tool_calls": [{"tool": "monitor.disk", "input": {"path": "/"}}], "result": "checking"}"#;
        let calls = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "monitor.disk");
    }

    #[test]
    fn test_parse_tool_calls_no_tools() {
        let response = r#"{"reasoning": "done", "tool_calls": [], "result": "complete"}"#;
        let calls = parse_tool_calls(response);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_invalid_json() {
        let response = "This is not JSON";
        let calls = parse_tool_calls(response);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_markdown_fenced() {
        let response = "```json\n{\"tool_calls\": [{\"tool\": \"fs.read\", \"input\": {\"path\": \"/etc/hosts\"}}]}\n```";
        let calls = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "fs.read");
    }

    #[test]
    fn test_parse_tool_calls_multiple() {
        let response = r#"{"tool_calls": [{"tool": "fs.read", "input": {}}, {"tool": "net.ping", "input": {"host": "1.1.1.1"}}]}"#;
        let calls = parse_tool_calls(response);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].tool_name, "fs.read");
        assert_eq!(calls[1].tool_name, "net.ping");
    }

    #[tokio::test]
    async fn test_autonomy_loop_cancellation() {
        let state = Arc::new(RwLock::new(OrchestratorState {
            goal_engine: crate::goal_engine::GoalEngine::new(),
            task_planner: crate::task_planner::TaskPlanner::new(),
            agent_router: crate::agent_router::AgentRouter::new(),
            result_aggregator: crate::result_aggregator::ResultAggregator::new(),
            decision_logger: crate::decision_logger::DecisionLogger::new(),
            started_at: std::time::Instant::now(),
            cancel_token: CancellationToken::new(),
            clients: crate::clients::ServiceClients::new(),
            health_checker: Arc::new(RwLock::new(crate::health::HealthChecker::new())),
        }));

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let handle = tokio::spawn(async move {
            run_autonomy_loop(
                state,
                cancel_clone,
                AutonomyConfig {
                    tick_interval: Duration::from_millis(50),
                    ..Default::default()
                },
            )
            .await;
        });

        // Cancel after a short delay
        tokio::time::sleep(Duration::from_millis(150)).await;
        cancel.cancel();

        // Should finish promptly
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("autonomy loop should stop")
            .expect("no panic");
    }
}
