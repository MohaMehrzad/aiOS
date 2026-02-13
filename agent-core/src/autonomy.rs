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

/// Data extracted from state for AI inference (allows dropping the write lock)
struct AiWorkItem {
    task: crate::proto::common::Task,
    task_id: String,
    goal_id: String,
    level: IntelligenceLevel,
    preferred_provider: String,
    messages: Vec<crate::goal_engine::GoalMessage>,
    clients: Arc<crate::clients::ServiceClients>,
}

/// Single tick of the autonomy loop
async fn autonomy_tick(
    state_arc: &Arc<RwLock<OrchestratorState>>,
    _config: &AutonomyConfig,
) -> anyhow::Result<()> {
    // ── Phase 1: Hold write lock for decomposition + task selection ──
    let ai_work = {
        let mut state = state_arc.write().await;

        // 1. Check goal engine for active goals
        let active_goals = state.goal_engine.active_goal_count();
        if active_goals == 0 {
            return Ok(());
        }

        debug!("Autonomy tick: {active_goals} active goals");

        // 2. Decompose pending goals that have no tasks yet,
        //    or advance pending goals that already have tasks (from submit_goal)
        let (pending_goals, _) = state.goal_engine.list_goals("pending", 10, 0).await;
        for goal in &pending_goals {
            let tasks = state.goal_engine.get_goal_tasks(&goal.id);
            if tasks.is_empty() {
                info!("Decomposing pending goal {} into tasks", goal.id);
                match state
                    .task_planner
                    .decompose_goal(&goal.id, &goal.description)
                    .await
                {
                    Ok(new_tasks) => {
                        let task_count = new_tasks.len();
                        state.goal_engine.add_tasks(&goal.id, new_tasks);
                        state.goal_engine.update_status(&goal.id, "in_progress");
                        info!(
                            "Goal {} decomposed into {task_count} tasks",
                            goal.id
                        );
                    }
                    Err(e) => {
                        error!("Failed to decompose goal {}: {e}", goal.id);
                    }
                }
            } else {
                // Tasks already exist (from submit_goal handler) — advance to in_progress
                state.goal_engine.update_status(&goal.id, "in_progress");
            }
        }

        // 3. Get next unblocked task from task planner
        let next_task = state.task_planner.next_task().cloned();
        let task = match next_task {
            Some(t) => t,
            None => return Ok(()),
        };

        let task_id = task.id.clone();
        let goal_id = task.goal_id.clone();
        let level = IntelligenceLevel::from_str(&task.intelligence_level);

        // Mark task as in-progress
        state.task_planner.mark_in_progress(&task_id);
        state.goal_engine.update_task_status(&goal_id, &task_id, "in_progress");

        // 4. Route task via agent router or handle directly
        let agent_id = state.agent_router.route_task(&task);

        if let Some(ref agent_id) = agent_id {
            info!("Dispatching task {task_id} to agent {agent_id}");
            state.agent_router.assign_task(agent_id, &task_id);

            state.decision_logger.log_decision(
                "task_routing",
                &[agent_id.clone()],
                "agent_dispatch",
                &format!(
                    "Task {task_id} dispatched to agent {agent_id} (level: {})",
                    level.as_str()
                ),
                level.as_str(),
                "heuristic",
            );

            // Agent polls via GetAssignedTask, executes, reports via ReportTaskResult.
            // Dead agent recovery at the end of this tick handles timeout.
            return Ok(());
        }

        // No local agent matched — try cluster routing if enabled
        if std::env::var("AIOS_CLUSTER_ENABLED").unwrap_or_default() == "true" {
            let cluster_guard = state.cluster.read().await;
            if let Some(remote_node_id) = state.agent_router.route_task_to_node(&task, &cluster_guard) {
                drop(cluster_guard);
                info!("Routing task {task_id} to remote node {remote_node_id}");

                let mut remote = crate::remote_exec::RemoteExecutor::new();
                match remote
                    .submit_remote_goal(
                        &remote_node_id,
                        &task.description,
                        5, // default priority
                        &format!("cluster:{}", task_id),
                    )
                    .await
                {
                    Ok(remote_goal_id) => {
                        info!(
                            "Task {task_id} submitted to remote node {remote_node_id} as goal {remote_goal_id}"
                        );
                        state.task_planner.complete_task(&task_id, Vec::new());
                        state
                            .goal_engine
                            .update_task_status(&goal_id, &task_id, "completed");
                        state.decision_logger.log_decision(
                            "task_routing",
                            &[remote_node_id],
                            "cluster_dispatch",
                            &format!("Task {task_id} routed to remote cluster node"),
                            level.as_str(),
                            "cluster",
                        );
                        return Ok(());
                    }
                    Err(e) => {
                        warn!("Remote dispatch failed for {task_id}: {e}, falling back to AI inference");
                    }
                }
            }
        }

        // No agent matched — prepare AI work item and release the lock
        let mut preferred_provider = get_preferred_provider(&state, &goal_id);
        let messages = state.goal_engine.get_messages(&goal_id);
        let clients = state.clients.clone(); // Arc clone — cheap

        if preferred_provider.is_empty() {
            preferred_provider = "qwen3".to_string();
        }

        Some(AiWorkItem {
            task,
            task_id,
            goal_id,
            level,
            preferred_provider,
            messages,
            clients,
        })
    }; // ── Write lock dropped here ──

    // ── Phase 2: AI inference WITHOUT holding the write lock ──
    if let Some(work) = ai_work {
        let backend = AiBackend::ApiGateway;
        info!(
            "Routing {} task {} to API gateway (provider: {})",
            work.level.as_str(),
            work.task_id,
            work.preferred_provider
        );

        let result = execute_ai_task(
            &work.clients,
            &work.task.description,
            work.level.as_str(),
            backend,
            &work.preferred_provider,
            &work.messages,
        )
        .await;

        // Execute tool calls WITHOUT holding the lock
        let tool_execution = execute_tool_calls_unlocked(
            &work.clients,
            &work.task_id,
            &result,
        )
        .await;

        // ── Phase 3: Reacquire write lock to record results ──
        let mut state = state_arc.write().await;

        record_ai_result(
            &mut state,
            &work.task_id,
            &work.goal_id,
            &work.task.description,
            work.level.as_str(),
            result,
            tool_execution,
        )
        .await;
    }

    // ── Phase 4: Brief write lock for housekeeping ──
    {
        let mut state = state_arc.write().await;

        // Check for stuck agent-assigned tasks (timeout recovery)
        let dead_agents = state.agent_router.dead_agents();
        for dead_id in &dead_agents {
            if let Some(stuck_task_id) = state.agent_router.get_assigned_task_id(dead_id) {
                warn!(
                    "Agent {dead_id} is dead with task {stuck_task_id} assigned — re-queuing task"
                );
                state.agent_router.task_completed(dead_id, false);
                state.task_planner.resume_task(&stuck_task_id);
            }
        }

        // Check if any goals are complete
        let (goals, _) = state.goal_engine.list_goals("", 100, 0).await;
        for goal in goals {
            if goal.status == "pending" || goal.status == "in_progress" {
                let progress = state.goal_engine.calculate_progress(&goal.id).await;
                if progress >= 100.0 {
                    state.goal_engine.update_status(&goal.id, "completed");
                    info!("Goal {} completed", goal.id);

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
    }

    Ok(())
}

/// Result of executing tool calls outside the write lock
struct ToolExecutionResult {
    tool_results: Vec<serde_json::Value>,
    all_succeeded: bool,
}

/// Execute tool calls from an AI response WITHOUT holding the state write lock.
/// Returns the results so they can be recorded once the lock is reacquired.
async fn execute_tool_calls_unlocked(
    clients: &crate::clients::ServiceClients,
    task_id: &str,
    result: &AiInferenceResult,
) -> ToolExecutionResult {
    if result.tool_calls.is_empty() || !result.success {
        return ToolExecutionResult {
            tool_results: Vec::new(),
            all_succeeded: true,
        };
    }

    let mut tool_results = Vec::new();
    let mut all_succeeded = true;

    for tc in &result.tool_calls {
        info!("Executing tool '{}' for task {task_id}", tc.tool_name);
        match execute_tool_call(clients, task_id, &tc.tool_name, &tc.input_json).await {
            Ok(tool_result) => {
                info!("Tool '{}' succeeded for task {task_id}", tc.tool_name);
                tool_results.push(tool_result);
            }
            Err(e) => {
                warn!("Tool '{}' failed for task {task_id}: {e}", tc.tool_name);
                all_succeeded = false;
                tool_results.push(serde_json::json!({
                    "tool": tc.tool_name,
                    "success": false,
                    "error": e.to_string(),
                }));
            }
        }
    }

    ToolExecutionResult {
        tool_results,
        all_succeeded,
    }
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
    conversation_history: &[crate::goal_engine::GoalMessage],
) -> AiInferenceResult {
    // Assemble context for the AI call
    let assembler = ContextAssembler::new(4096);
    let context = assembler.assemble_for_task(task_description, intelligence_level, &[], &[]);
    let mut system_prompt = match &context {
        Ok(ctx) => ctx.system_prompt.clone(),
        Err(_) => "You are aiOS, an AI-native operating system that executes system tasks by calling tools.".to_string(),
    };

    // Critical: Tell the model in the system prompt to output JSON.
    // Many models (Qwen3, etc.) follow system prompt instructions more reliably.
    system_prompt.push_str(
        "\n\nYou MUST always respond with ONLY a valid JSON object. \
         Never output natural language, markdown, or explanations outside of JSON. \
         Your response must contain a \"tool_calls\" array with at least one tool to execute."
    );

    // Query memory service for relevant context chunks
    match clients.memory().await {
        Ok(mut mem_client) => {
            let mem_request = tonic::Request::new(crate::proto::memory::ContextRequest {
                task_description: task_description.to_string(),
                max_tokens: 2048,
                memory_tiers: vec![
                    "operational".to_string(),
                    "working".to_string(),
                    "long_term".to_string(),
                ],
            });
            match mem_client.assemble_context(mem_request).await {
                Ok(response) => {
                    let chunks = response.into_inner().chunks;
                    if !chunks.is_empty() {
                        let mut memory_context = String::from("\n\nRelevant memory context:\n");
                        for chunk in &chunks {
                            memory_context.push_str(&format!(
                                "- [{}] {}\n",
                                chunk.source, chunk.content
                            ));
                        }
                        system_prompt.push_str(&memory_context);
                        info!("Assembled {} memory chunks for task context", chunks.len());
                    }
                }
                Err(e) => {
                    debug!("Memory context assembly unavailable: {e}");
                }
            }
        }
        Err(e) => {
            debug!("Memory service unavailable for context: {e}");
        }
    }

    let mut prompt = format!("Task: {task_description}\n\n");

    // Include conversation history for context (e.g., after user replies to clarification)
    let relevant_messages: Vec<_> = conversation_history
        .iter()
        .filter(|m| m.sender == "user" || m.sender == "ai")
        .collect();
    if !relevant_messages.is_empty() {
        prompt.push_str("Previous conversation:\n");
        for msg in relevant_messages {
            let label = if msg.sender == "user" { "[User]" } else { "[AI]" };
            prompt.push_str(&format!("{}: {}\n", label, msg.content));
        }
        prompt.push_str("\nExecute the task using the provided context.\n\n");
    }

    // Tell the AI what tools are available — dynamically queried from the tool registry
    let tool_catalog = query_tool_catalog(clients).await;
    prompt.push_str(&tool_catalog);

    prompt.push_str(
        "IMPORTANT — Self-Evolution:\n\
         If the task requires a tool you do NOT have, create one using plugin.create.\n\
         The code must define: def main(input_data: dict) -> dict\n\n"
    );

    prompt.push_str(
        "You MUST respond with ONLY a valid JSON object. No prose, no markdown, no explanation outside JSON.\n\n\
         FORMAT — Execute tools:\n\
         {\"reasoning\": \"brief explanation\", \"tool_calls\": [{\"tool\": \"monitor.cpu\", \"input\": {}}, {\"tool\": \"monitor.memory\", \"input\": {}}], \"result\": \"summary of what will be done\"}\n\n\
         FORMAT — Need user input:\n\
         {\"needs_clarification\": true, \"questions\": [\"What specific thing?\"]}\n\n\
         FORMAT — Create new tool then use it:\n\
         {\"reasoning\": \"Need custom tool\", \"tool_calls\": [{\"tool\": \"plugin.create\", \"input\": {\"name\": \"my_tool\", \"description\": \"Does X\", \"code\": \"def main(input_data):\\n    return {'result': 'done'}\", \"capabilities\": [], \"dependencies\": []}}, {\"tool\": \"plugin.my_tool\", \"input\": {}}], \"result\": \"Created and executed tool\"}\n\n\
         EXAMPLE — Check CPU usage:\n\
         {\"reasoning\": \"Using monitor.cpu to get CPU metrics\", \"tool_calls\": [{\"tool\": \"monitor.cpu\", \"input\": {}}], \"result\": \"Checking CPU usage\"}\n\n\
         RULES:\n\
         1. ALWAYS include tool_calls array with at least one tool call — never just describe a plan\n\
         2. Output ONLY valid JSON — no text before or after\n\
         3. Tool names use namespace.action format (e.g. monitor.cpu, fs.read, net.ping)\n\
         4. If unsure which tool, use the closest match from the catalog above"
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
            // API gateway already tried all providers (qwen3/claude/openai).
            // Local runtime (TinyLlama) can't handle tool-calling prompts.
            // Skip to heuristic fallback.
            info!("API gateway exhausted all providers, skipping local runtime");
            None
        }
    };

    if let Some(r) = fallback {
        return r;
    }

    // Final fallback: all backends failed — mark as failure so the task
    // is NOT silently marked completed with zero work done.
    warn!("All AI backends unavailable, task will be marked as failed");
    AiInferenceResult {
        success: false,
        response_text: "All AI backends are currently unavailable. The task could not be executed.".to_string(),
        tool_calls: vec![],
        model_used: "none".to_string(),
        tokens_used: 0,
    }
}

/// Query the live tool catalog from the tools gRPC service.
/// Groups tools by namespace and formats them for the AI prompt.
/// Falls back to a static list if the tools service is unreachable.
async fn query_tool_catalog(clients: &crate::clients::ServiceClients) -> String {
    match clients.tools().await {
        Ok(mut client) => {
            let request = tonic::Request::new(crate::proto::tools::ListToolsRequest {
                namespace: String::new(),
            });
            match client.list_tools(request).await {
                Ok(response) => {
                    let tools = response.into_inner().tools;
                    if tools.is_empty() {
                        return static_tool_catalog();
                    }

                    // Group by namespace
                    let mut by_ns: std::collections::BTreeMap<String, Vec<String>> =
                        std::collections::BTreeMap::new();
                    for tool in &tools {
                        let ns = if tool.namespace.is_empty() {
                            "other".to_string()
                        } else {
                            tool.namespace.clone()
                        };
                        let desc = if tool.description.is_empty() {
                            tool.name.clone()
                        } else {
                            format!("{} — {}", tool.name, tool.description)
                        };
                        by_ns.entry(ns).or_default().push(desc);
                    }

                    let mut catalog = format!("Available tools ({} total):\n", tools.len());
                    for (ns, tool_list) in &by_ns {
                        catalog.push_str(&format!("[{}] {}\n", ns, tool_list.join(", ")));
                    }
                    catalog.push('\n');
                    catalog
                }
                Err(e) => {
                    debug!("Failed to list tools via gRPC: {e}");
                    static_tool_catalog()
                }
            }
        }
        Err(e) => {
            debug!("Cannot connect to tools service for catalog: {e}");
            static_tool_catalog()
        }
    }
}

/// Static fallback tool catalog when tools service is unreachable
fn static_tool_catalog() -> String {
    "Available tools you can call:\n\
     - fs.read, fs.write, fs.list, fs.delete, fs.mkdir, fs.copy, fs.move, fs.stat, fs.search\n\
     - process.list, process.kill, process.spawn, process.info\n\
     - service.list, service.start, service.stop, service.restart, service.status\n\
     - net.ping, net.dns, net.interfaces, net.http_get, net.port_scan\n\
     - firewall.rules, firewall.add_rule, firewall.delete_rule\n\
     - pkg.install, pkg.remove, pkg.list_installed, pkg.search, pkg.update\n\
     - sec.check_perms, sec.audit_query\n\
     - monitor.cpu, monitor.memory, monitor.disk, monitor.network, monitor.logs\n\
     - web.http_request, web.scrape, web.webhook, web.download, web.api_call\n\
     - git.init, git.clone, git.add, git.commit, git.push, git.pull, git.branch, git.status, git.log, git.diff\n\
     - code.scaffold, code.generate\n\
     - self.inspect, self.health, self.update, self.rebuild\n\
     - plugin.create, plugin.list, plugin.delete, plugin.install_deps\n\n"
        .to_string()
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
                max_tokens: 2048,
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
                max_tokens: 60000,
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
    let text_len = response_text.len();

    // Use the robust JSON extractor that handles prose wrappers, markdown fences, etc.
    let parsed = match extract_json_from_text(response_text) {
        Some(v) => v,
        None => {
            // Log why parsing failed for debugging
            let preview: String = response_text.chars().take(200).collect();
            let suffix: String = response_text.chars().rev().take(100).collect::<String>().chars().rev().collect();
            tracing::warn!(
                "parse_tool_calls: JSON extraction failed (len={text_len}). \
                 Start: {preview:?}... End: ...{suffix:?}"
            );
            // Try direct serde parse to get error message
            if let Err(e) = serde_json::from_str::<serde_json::Value>(response_text.trim()) {
                tracing::warn!("parse_tool_calls: serde error: {e}");
            }
            return calls;
        }
    };

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
    } else {
        // Fallback: try to extract tool calls from "steps", "tools_needed", or "actions" arrays
        let extracted = extract_tools_from_json_fallback(&parsed);
        if !extracted.is_empty() {
            tracing::info!("parse_tool_calls: extracted {} tool calls from JSON fallback fields", extracted.len());
            return extracted;
        }

        let keys: Vec<&str> = parsed.as_object()
            .map(|o| o.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        tracing::warn!("parse_tool_calls: JSON parsed OK but no tool_calls array. Keys: {keys:?}");
    }

    // If no tool calls found from JSON, try natural language extraction as last resort
    if calls.is_empty() {
        let nl_calls = extract_tools_from_natural_language(response_text);
        if !nl_calls.is_empty() {
            tracing::info!("parse_tool_calls: extracted {} tool calls from natural language", nl_calls.len());
            return nl_calls;
        }
    }

    calls
}

/// Execute a single tool call via the tools gRPC service
async fn execute_tool_call(
    clients: &crate::clients::ServiceClients,
    task_id: &str,
    tool_name: &str,
    input_json: &[u8],
) -> anyhow::Result<serde_json::Value> {
    let mut client = clients
        .tools()
        .await
        .map_err(|e| anyhow::anyhow!("Cannot connect to tools service: {e}"))?;

    let request = tonic::Request::new(crate::proto::tools::ExecuteRequest {
        tool_name: tool_name.to_string(),
        agent_id: "autonomy-loop".to_string(),
        task_id: task_id.to_string(),
        input_json: input_json.to_vec(),
        reason: format!("Autonomy loop executing tool for task {task_id}"),
    });

    let response = client
        .execute(request)
        .await
        .map_err(|e| anyhow::anyhow!("Tool execution gRPC failed: {e}"))?;

    let resp = response.into_inner();

    if resp.success {
        let output: serde_json::Value = serde_json::from_slice(&resp.output_json)
            .unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(&resp.output_json).to_string())
            });
        Ok(serde_json::json!({
            "tool": tool_name,
            "success": true,
            "output": output,
            "execution_id": resp.execution_id,
            "duration_ms": resp.duration_ms,
        }))
    } else {
        Err(anyhow::anyhow!("Tool '{}' failed: {}", tool_name, resp.error))
    }
}

/// Parse clarification request from AI response
fn parse_clarification(response_text: &str) -> Option<String> {
    let parsed = extract_json_from_text(response_text)?;

    if parsed.get("needs_clarification").and_then(|v| v.as_bool()) == Some(true) {
        if let Some(questions) = parsed.get("questions").and_then(|v| v.as_array()) {
            let q_text: Vec<String> = questions
                .iter()
                .enumerate()
                .filter_map(|(i, q)| q.as_str().map(|s| format!("{}. {}", i + 1, s)))
                .collect();
            if !q_text.is_empty() {
                return Some(q_text.join("\n"));
            }
        }
        if let Some(reasoning) = parsed.get("reasoning").and_then(|v| v.as_str()) {
            return Some(reasoning.to_string());
        }
        return Some("I need more information to proceed with this task.".to_string());
    }
    None
}

/// Try to find and extract a JSON object from text that may contain prose around it.
/// Handles: raw JSON, markdown-fenced JSON, JSON embedded in prose like "Response:\n\n```json\n{...}\n```"
fn extract_json_from_text(text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();

    // 1. Try direct parse
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(v);
    }

    // 2. Try extracting from markdown code fences (anywhere in text)
    if let Some(fence_start) = trimmed.find("```") {
        let after_fence = &trimmed[fence_start + 3..];
        // Skip optional language tag (e.g. ```json)
        let json_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[json_start..];
        if let Some(fence_end) = content.find("```") {
            let inside = content[..fence_end].trim();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(inside) {
                return Some(v);
            }
        }
    }

    // 3. Try finding the first '{' and matching closing '}'
    if let Some(start) = trimmed.find('{') {
        // Walk forward to find the matching closing brace
        let candidate = &trimmed[start..];
        let mut depth = 0i32;
        let mut end_pos = 0;
        for (i, ch) in candidate.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end_pos > 0 {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&candidate[..end_pos]) {
                return Some(v);
            }
        }
    }

    None
}

/// Extract tool calls from JSON fields that aren't "tool_calls" (e.g. "steps", "tools_needed", "actions").
/// Some models output plans in alternative formats — this recovers tool names from those.
fn extract_tools_from_json_fallback(parsed: &serde_json::Value) -> Vec<ToolCallRequest> {
    let mut calls = Vec::new();

    // First, look for tool parameters that might be at the top level or under known keys.
    // Some models put parameters in "parameters", "inputs", "arguments", or "input" at root level.
    let extra_params = find_tool_parameters(parsed);

    // Try "steps" array — each step might reference a tool
    if let Some(steps) = parsed.get("steps").and_then(|v| v.as_array()) {
        for step in steps {
            // Step might be {"tool": "monitor.cpu", ...} or {"action": "email.send", "parameters": {...}}
            let tool_name = step.get("tool").and_then(|v| v.as_str())
                .or_else(|| step.get("action").and_then(|v| v.as_str()))
                .or_else(|| step.get("name").and_then(|v| v.as_str()));
            if let Some(tool) = tool_name {
                let input = step.get("input")
                    .or_else(|| step.get("parameters"))
                    .or_else(|| step.get("params"))
                    .or_else(|| step.get("arguments"))
                    .or_else(|| step.get("args"))
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                if let Ok(bytes) = serde_json::to_vec(&input) {
                    calls.push(ToolCallRequest {
                        tool_name: tool.to_string(),
                        input_json: bytes,
                    });
                }
            } else {
                // Try to extract tool name from step description text
                let text = step.as_str()
                    .or_else(|| step.get("task").and_then(|v| v.as_str()))
                    .or_else(|| step.get("description").and_then(|v| v.as_str()))
                    .unwrap_or("");
                calls.extend(extract_tools_from_natural_language(text));
            }
        }
    }

    // Try "tools_needed" array
    if calls.is_empty() {
        if let Some(tools) = parsed.get("tools_needed").and_then(|v| v.as_array()) {
            for tool in tools {
                // Tool might be a string name or an object with name + params
                if let Some(obj) = tool.as_object() {
                    let name = obj.get("tool").or_else(|| obj.get("name"))
                        .and_then(|v| v.as_str()).unwrap_or("");
                    let input = obj.get("input").or_else(|| obj.get("parameters"))
                        .cloned().unwrap_or(serde_json::json!({}));
                    if !name.is_empty() {
                        if let Ok(bytes) = serde_json::to_vec(&input) {
                            calls.push(ToolCallRequest {
                                tool_name: name.to_string(),
                                input_json: bytes,
                            });
                        }
                    }
                } else if let Some(name) = tool.as_str() {
                    // Use extra_params if available for this tool
                    let input = extra_params.get(name)
                        .cloned()
                        .unwrap_or(serde_json::json!({}));
                    if let Ok(bytes) = serde_json::to_vec(&input) {
                        calls.push(ToolCallRequest {
                            tool_name: name.to_string(),
                            input_json: bytes,
                        });
                    }
                }
            }
        }
    }

    // Try "actions" array
    if calls.is_empty() {
        if let Some(actions) = parsed.get("actions").and_then(|v| v.as_array()) {
            for action in actions {
                let tool_name = action.get("tool").and_then(|v| v.as_str())
                    .or_else(|| action.get("name").and_then(|v| v.as_str()));
                if let Some(tool) = tool_name {
                    let input = action.get("input")
                        .or_else(|| action.get("parameters"))
                        .or_else(|| action.get("params"))
                        .cloned()
                        .unwrap_or(serde_json::json!({}));
                    if let Ok(bytes) = serde_json::to_vec(&input) {
                        calls.push(ToolCallRequest {
                            tool_name: tool.to_string(),
                            input_json: bytes,
                        });
                    }
                }
            }
        }
    }

    // Post-process: if any calls have empty {} input, try to enrich from extra_params
    for call in &mut calls {
        if call.input_json == b"{}" {
            if let Some(params) = extra_params.get(&call.tool_name) {
                if let Ok(bytes) = serde_json::to_vec(params) {
                    tracing::info!(
                        "Enriched tool call {} with parameters from response JSON",
                        call.tool_name
                    );
                    call.input_json = bytes;
                }
            }
        }
    }

    calls
}

/// Scan the parsed JSON for tool parameters that might be stored under various keys.
/// Returns a map of tool_name -> parameters found.
fn find_tool_parameters(parsed: &serde_json::Value) -> std::collections::HashMap<String, serde_json::Value> {
    let mut params = std::collections::HashMap::new();

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return params,
    };

    // Look for "parameters", "inputs", "arguments", "input" at root level
    for key in &["parameters", "inputs", "arguments", "input", "params", "tool_input"] {
        if let Some(val) = obj.get(*key) {
            if let Some(inner_obj) = val.as_object() {
                // If this contains a "tool" key, it's tool-specific: {"tool": "email.send", "to": "..."}
                if let Some(tool_name) = inner_obj.get("tool").and_then(|v| v.as_str()) {
                    let mut clean = val.clone();
                    if let Some(m) = clean.as_object_mut() {
                        m.remove("tool");
                    }
                    params.insert(tool_name.to_string(), clean);
                } else {
                    // Generic parameters — associate with any tool that needs them
                    // Store under a wildcard key "*"
                    params.insert("*".to_string(), val.clone());
                }
            }
        }
    }

    // Look for "tool_calls" that might be structured differently
    if let Some(tc) = obj.get("tool_calls").and_then(|v| v.as_array()) {
        for item in tc {
            if let Some(item_obj) = item.as_object() {
                let name = item_obj.get("tool")
                    .or_else(|| item_obj.get("name"))
                    .or_else(|| item_obj.get("function"))
                    .and_then(|v| v.as_str());
                let input = item_obj.get("input")
                    .or_else(|| item_obj.get("parameters"))
                    .or_else(|| item_obj.get("params"))
                    .or_else(|| item_obj.get("arguments"));
                if let (Some(n), Some(i)) = (name, input) {
                    params.insert(n.to_string(), i.clone());
                }
            }
        }
    }

    // If wildcard params found and we have tools_needed, associate with those tools
    if let Some(wildcard) = params.remove("*") {
        if let Some(tools) = obj.get("tools_needed").and_then(|v| v.as_array()) {
            for tool in tools {
                if let Some(name) = tool.as_str() {
                    params.entry(name.to_string()).or_insert_with(|| wildcard.clone());
                }
            }
        }
    }

    params
}

/// Extract tool names from natural language text.
/// Matches patterns like "Call monitor.cpu", "use fs.read", "execute net.ping".
fn extract_tools_from_natural_language(text: &str) -> Vec<ToolCallRequest> {
    let mut calls = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Pattern: "Call tool.name" or "call tool.name" or "Use tool.name" or "Execute tool.name"
    // Tool names follow the pattern: namespace.action (e.g., monitor.cpu, fs.read, net.ping)
    for word_idx in 0..text.len() {
        let remaining = &text[word_idx..];
        // Look for known trigger words followed by a tool name pattern
        let trigger_prefixes = ["call ", "Call ", "use ", "Use ", "execute ", "Execute ", "run ", "Run "];
        for prefix in &trigger_prefixes {
            if remaining.starts_with(prefix) {
                let after = &remaining[prefix.len()..];
                // Extract potential tool name: word.word pattern
                if let Some(tool_name) = extract_tool_name_at_start(after) {
                    if !seen.contains(&tool_name) {
                        seen.insert(tool_name.clone());
                        calls.push(ToolCallRequest {
                            tool_name,
                            input_json: b"{}".to_vec(),
                        });
                    }
                }
            }
        }
    }

    // Also scan for standalone tool name patterns (namespace.action)
    // This catches: "monitor.cpu and monitor.memory tools"
    if calls.is_empty() {
        let known_namespaces = [
            "fs", "process", "service", "net", "firewall", "pkg",
            "sec", "monitor", "hw", "web", "git", "code", "self", "plugin", "container",
        ];
        for ns in &known_namespaces {
            let prefix = format!("{}.", ns);
            let mut search_from = 0;
            while let Some(pos) = text[search_from..].find(&prefix) {
                let abs_pos = search_from + pos;
                if let Some(tool_name) = extract_tool_name_at_start(&text[abs_pos..]) {
                    if !seen.contains(&tool_name) {
                        seen.insert(tool_name.clone());
                        calls.push(ToolCallRequest {
                            tool_name,
                            input_json: b"{}".to_vec(),
                        });
                    }
                }
                search_from = abs_pos + prefix.len();
            }
        }
    }

    calls
}

/// Extract a tool name (namespace.action) from the start of text.
/// Returns None if the text doesn't start with a valid tool name pattern.
fn extract_tool_name_at_start(text: &str) -> Option<String> {
    let text = text.trim_start_matches('`');
    let mut chars = text.chars().peekable();
    let mut name = String::new();
    let mut has_dot = false;

    while let Some(&ch) = chars.peek() {
        if ch.is_alphanumeric() || ch == '_' || (ch == '.' && !has_dot) {
            if ch == '.' {
                has_dot = true;
            }
            name.push(ch);
            chars.next();
        } else {
            break;
        }
    }

    // Must have namespace.action format
    if has_dot && name.len() >= 3 {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(name);
        }
    }
    None
}

/// Convert a JSON value into a human-readable summary.
/// Handles our expected format (reasoning/result/questions) plus arbitrary structures.
fn json_to_readable(parsed: &serde_json::Value) -> String {
    let mut parts = Vec::new();

    // Our expected fields
    if let Some(reasoning) = parsed.get("reasoning").and_then(|v| v.as_str()) {
        if !reasoning.is_empty() {
            parts.push(reasoning.to_string());
        }
    }
    if let Some(result) = parsed.get("result").and_then(|v| v.as_str()) {
        if !result.is_empty() {
            parts.push(result.to_string());
        }
    }
    if let Some(questions) = parsed.get("questions").and_then(|v| v.as_array()) {
        let q_text: Vec<String> = questions
            .iter()
            .enumerate()
            .filter_map(|(i, q)| q.as_str().map(|s| format!("{}. {}", i + 1, s)))
            .collect();
        if !q_text.is_empty() {
            parts.push(q_text.join("\n"));
        }
    }

    // Handle "needs_clarification" responses with inline questions
    if parsed.get("needs_clarification").and_then(|v| v.as_bool()) == Some(true) && parts.is_empty() {
        parts.push("I need some more information before I can proceed:".to_string());
    }

    // Handle "steps" array (some models return a plan)
    if let Some(steps) = parsed.get("steps").and_then(|v| v.as_array()) {
        let step_text: Vec<String> = steps
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                // Each step might be a string or an object with "task"/"description"
                if let Some(text) = s.as_str() {
                    Some(format!("{}. {}", i + 1, text))
                } else {
                    let task = s.get("task").or_else(|| s.get("description")).and_then(|v| v.as_str());
                    task.map(|t| format!("{}. {}", i + 1, t))
                }
            })
            .collect();
        if !step_text.is_empty() {
            parts.push(format!("Plan:\n{}", step_text.join("\n")));
        }
    }

    // Handle "message" / "response" / "answer" / "explanation" (common model outputs)
    for key in &["message", "response", "answer", "explanation", "summary", "output"] {
        if let Some(val) = parsed.get(*key).and_then(|v| v.as_str()) {
            if !val.is_empty() && !parts.iter().any(|p| p.contains(val)) {
                parts.push(val.to_string());
            }
        }
    }

    // Handle "tool_calls" — summarize what the AI wants to do
    if let Some(tool_calls) = parsed.get("tool_calls").and_then(|v| v.as_array()) {
        if !tool_calls.is_empty() {
            let tc_text: Vec<String> = tool_calls
                .iter()
                .filter_map(|tc| tc.get("tool").and_then(|v| v.as_str()).map(|t| format!("- {}", t)))
                .collect();
            if !tc_text.is_empty() {
                parts.push(format!("Actions planned:\n{}", tc_text.join("\n")));
            }
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    let combined = parts.join("\n\n");
    if combined.len() > 2000 {
        format!("{}...", &combined[..2000])
    } else {
        combined
    }
}

/// Extract readable display text from an AI response (may be JSON or plain text)
fn extract_ai_display_text(response_text: &str) -> String {
    let text = response_text.trim();
    if text.is_empty() {
        return String::new();
    }

    // Try to find and parse JSON from the response
    if let Some(parsed) = extract_json_from_text(text) {
        let readable = json_to_readable(&parsed);
        if !readable.is_empty() {
            // If there was prose before the JSON, prepend it
            let prose_before = if let Some(brace_pos) = text.find('{') {
                let before = text[..brace_pos].trim();
                // Strip "Response:", "JSON Object:", markdown fences, etc.
                let cleaned: String = before
                    .lines()
                    .filter(|l| {
                        let t = l.trim().to_lowercase();
                        !t.is_empty()
                            && !t.starts_with("```")
                            && !t.starts_with("response")
                            && !t.starts_with("json")
                            && !t.starts_with("here")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if cleaned.is_empty() { None } else { Some(cleaned) }
            } else {
                None
            };

            return match prose_before {
                Some(prose) => format!("{}\n\n{}", prose, readable),
                None => readable,
            };
        }
    }

    // Not JSON or unrecognized structure — return as plain text (truncated)
    if text.len() > 2000 {
        format!("{}...", &text[..2000])
    } else {
        text.to_string()
    }
}

/// Produce a short human-readable summary of a tool's output.
/// Never dumps raw code or large JSON blobs into chat.
fn summarize_tool_output(tool_name: &str, output: Option<&serde_json::Value>) -> String {
    let output = match output {
        Some(v) => v,
        None => return "completed successfully".to_string(),
    };

    // Plugin tools — extract name + description, skip code
    if tool_name.starts_with("plugin.create") || tool_name == "plugin.create" {
        let name = output.get("name").and_then(|v| v.as_str())
            .or_else(|| output.get("plugin_name").and_then(|v| v.as_str()));
        let desc = output.get("description").and_then(|v| v.as_str());
        return match (name, desc) {
            (Some(n), Some(d)) => format!("Created plugin '{n}' — {d}"),
            (Some(n), None) => format!("Created plugin '{n}'"),
            _ => "Plugin created successfully".to_string(),
        };
    }

    // Plugin execution — extract the meaningful result
    if tool_name.starts_with("plugin.") {
        let plugin_name = tool_name.strip_prefix("plugin.").unwrap_or(tool_name);
        if let Some(result) = output.get("result") {
            let s = match result {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            };
            let truncated = if s.len() > 300 { format!("{}...", &s[..300]) } else { s };
            return format!("'{plugin_name}' returned: {truncated}");
        }
        return format!("'{plugin_name}' completed successfully").to_string();
    }

    // For known tool namespaces, produce brief summaries
    match tool_name {
        // Filesystem
        t if t.starts_with("fs.") => {
            if let Some(path) = output.get("path").and_then(|v| v.as_str()) {
                return format!("OK ({path})");
            }
            "OK".to_string()
        }
        // Web tools — extract URL or status
        t if t.starts_with("web.") => {
            let url = output.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let status = output.get("status").or_else(|| output.get("status_code"));
            let body_len = output.get("body").and_then(|v| v.as_str()).map(|s| s.len());
            let mut summary = String::new();
            if !url.is_empty() {
                summary.push_str(url);
            }
            if let Some(s) = status {
                summary.push_str(&format!(" (status: {s})"));
            }
            if let Some(len) = body_len {
                summary.push_str(&format!(" [{len} chars]"));
            }
            if summary.is_empty() { "OK".to_string() } else { summary }
        }
        // Service management
        t if t.starts_with("service.") || t.starts_with("process.") => {
            if let Some(msg) = output.get("message").and_then(|v| v.as_str()) {
                return msg.to_string();
            }
            "OK".to_string()
        }
        // Default: extract a "message", "result", or "status" field; otherwise "OK"
        _ => {
            for key in &["message", "result", "status", "output"] {
                if let Some(val) = output.get(*key) {
                    let s = match val {
                        serde_json::Value::String(s) => s.clone(),
                        other => serde_json::to_string(other).unwrap_or_default(),
                    };
                    let truncated = if s.len() > 200 { format!("{}...", &s[..200]) } else { s };
                    return truncated;
                }
            }
            "completed successfully".to_string()
        }
    }
}

/// Build a human-readable summary from the AI response and tool execution results.
/// This gets posted as an "ai" message so users can see what the AI reasoned and
/// what tool outputs were produced, instead of just "Task completed".
fn build_completion_summary(response_text: &str, tool_results: &[serde_json::Value]) -> String {
    let mut parts = Vec::new();

    // Extract readable AI reasoning from the response (handles prose-wrapped JSON, fences, etc.)
    if let Some(parsed) = extract_json_from_text(response_text) {
        if let Some(reasoning) = parsed.get("reasoning").and_then(|v| v.as_str()) {
            if !reasoning.is_empty() {
                parts.push(reasoning.to_string());
            }
        }
        if let Some(result) = parsed.get("result").and_then(|v| v.as_str()) {
            if !result.is_empty() {
                parts.push(result.to_string());
            }
        }
        // If our expected fields aren't present, use the generic readable extractor
        if parts.is_empty() {
            let readable = json_to_readable(&parsed);
            if !readable.is_empty() {
                parts.push(readable);
            }
        }
    }

    // Summarize tool execution results — brief human-readable, never raw code/JSON
    for tr in tool_results {
        let tool_name = tr.get("tool").and_then(|v| v.as_str()).unwrap_or("unknown");
        let success = tr.get("success").and_then(|v| v.as_bool()).unwrap_or(false);

        if success {
            let summary = summarize_tool_output(tool_name, tr.get("output"));
            parts.push(format!("**{tool_name}**: {summary}"));
        } else {
            let err = tr.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
            parts.push(format!("**{tool_name}** failed: {err}"));
        }
    }

    let combined = parts.join("\n\n");
    // Cap total length
    if combined.len() > 3000 {
        format!("{}...", &combined[..3000])
    } else {
        combined
    }
}

/// Record the result of AI inference + tool execution into state.
/// Called AFTER tool execution completes, while holding the write lock.
/// Tool execution happens outside the lock via execute_tool_calls_unlocked().
async fn record_ai_result(
    state: &mut OrchestratorState,
    task_id: &str,
    goal_id: &str,
    task_description: &str,
    intelligence_level: &str,
    result: AiInferenceResult,
    tool_exec: ToolExecutionResult,
) {
    // Log what the AI returned for debugging
    let tool_count = result.tool_calls.len();
    let response_preview: String = result.response_text.chars().take(200).collect();
    info!(
        "Task {task_id}: AI returned {} tool calls, {} tokens, model={}, response preview: {}",
        tool_count, result.tokens_used, result.model_used, response_preview
    );

    // If the AI inference itself failed (all backends down), mark the task
    // as failed rather than silently succeeding or waiting for input.
    if !result.success && result.tool_calls.is_empty() {
        let error_msg = if result.response_text.is_empty() {
            "AI inference failed — all backends unavailable".to_string()
        } else {
            result.response_text.clone()
        };

        state.task_planner.fail_task(task_id, &error_msg);
        state
            .goal_engine
            .update_task_status(goal_id, task_id, "failed");
        state.goal_engine.add_message(
            goal_id,
            "system",
            &format!("Task failed: {error_msg}"),
        );

        state.result_aggregator.record_result(
            goal_id,
            crate::proto::common::TaskResult {
                task_id: task_id.to_string(),
                success: false,
                output_json: vec![],
                error: error_msg,
                duration_ms: 0,
                tokens_used: result.tokens_used,
                model_used: result.model_used.clone(),
            },
        );

        warn!("Task {task_id} failed: AI inference unsuccessful");
        return;
    }

    // If AI returned zero tool calls, NEVER auto-complete.
    // An OS should DO things — no tools executed means no work was done.
    if result.tool_calls.is_empty() {
        // Count how many times we've already asked for input on this goal
        // to prevent infinite awaiting_input loops
        let ai_msg_count = state.goal_engine.get_messages(goal_id)
            .iter()
            .filter(|m| m.sender == "ai")
            .count();

        if ai_msg_count >= 3 {
            // Too many retries — fail the task instead of looping forever
            let error_msg = "AI was unable to produce executable tool calls after multiple attempts. \
                             The model may not support the required JSON output format.";
            state.task_planner.fail_task(task_id, error_msg);
            state.goal_engine.update_task_status(goal_id, task_id, "failed");
            state.goal_engine.add_message(
                goal_id,
                "system",
                &format!("Task failed: {error_msg}"),
            );
            warn!("Task {task_id}: Failed after {ai_msg_count} attempts without tool calls");
            return;
        }

        let ai_text = extract_ai_display_text(&result.response_text);

        if let Some(clarification) = parse_clarification(&result.response_text) {
            state.goal_engine.add_message(goal_id, "ai", &clarification);
        } else if !ai_text.is_empty() {
            state.goal_engine.add_message(goal_id, "ai", &ai_text);
        } else {
            state.goal_engine.add_message(
                goal_id,
                "ai",
                "I received this task but wasn't able to determine what actions to take. Please provide more specific instructions.",
            );
        }

        state.task_planner.mark_awaiting_input(task_id);
        state
            .goal_engine
            .update_task_status(goal_id, task_id, "awaiting_input");

        info!("Task {task_id}: No tools executed, awaiting user input (attempt {ai_msg_count})");
        return;
    }

    // Record tool execution results (tools were already executed outside the lock)
    let ToolExecutionResult {
        tool_results,
        all_succeeded,
    } = tool_exec;

    if !all_succeeded {
        let error_msg = tool_results
            .iter()
            .filter(|r| r.get("success").and_then(|v| v.as_bool()) == Some(false))
            .filter_map(|r| r.get("error").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("; ");

        state.task_planner.fail_task(task_id, &error_msg);
        state
            .goal_engine
            .update_task_status(goal_id, task_id, "failed");
        state.goal_engine.add_message(
            goal_id,
            "system",
            &format!("Task failed: {error_msg}"),
        );

        state.result_aggregator.record_result(
            goal_id,
            crate::proto::common::TaskResult {
                task_id: task_id.to_string(),
                success: false,
                output_json: serde_json::to_vec(&tool_results).unwrap_or_default(),
                error: error_msg,
                duration_ms: 0,
                tokens_used: result.tokens_used,
                model_used: result.model_used.clone(),
            },
        );

        state.decision_logger.log_decision(
            "ai_execution",
            &[task_id.to_string()],
            "failed",
            &format!(
                "Task '{}' failed during tool execution",
                task_description
            ),
            intelligence_level,
            "ai",
        );

        warn!("Task {task_id} failed during tool execution");
        return;
    }

    // All tools succeeded — build combined output
    let output = serde_json::to_vec(&serde_json::json!({
        "ai_response": result.response_text,
        "tool_results": tool_results,
        "model_used": result.model_used,
    }))
    .unwrap_or_else(|_| b"{}".to_vec());

    // Post AI summary (reasoning + tool results) so the user sees what happened
    let ai_summary = build_completion_summary(&result.response_text, &tool_results);
    if !ai_summary.is_empty() {
        state.goal_engine.add_message(goal_id, "ai", &ai_summary);
    }

    // Add completion message to goal
    state.goal_engine.add_message(
        goal_id,
        "system",
        &format!("Task completed: {task_description}"),
    );

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
            clients: Arc::new(crate::clients::ServiceClients::new()),
            health_checker: Arc::new(RwLock::new(crate::health::HealthChecker::new())),
            cluster: Arc::new(RwLock::new(crate::cluster::ClusterManager::new("test"))),
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
