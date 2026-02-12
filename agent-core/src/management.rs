//! Management Console — REST API, WebSocket, Chat, and Dashboard
//!
//! Provides HTTP endpoints for monitoring and controlling aiOS.
//! Includes WebSocket endpoint for real-time updates.
//! Chat endpoint for direct AI interaction.
//! Runs on port 9090 alongside the gRPC server.

use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{
    extract::{Path, State},
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::health::HealthChecker;
use crate::OrchestratorState;

type SharedState = Arc<RwLock<OrchestratorState>>;

/// Combined state for management server
#[derive(Clone)]
struct MgmtState {
    orchestrator: SharedState,
    health_checker: Arc<RwLock<HealthChecker>>,
}

/// Start the management HTTP server on port 9090
pub async fn start_management_server(
    state: SharedState,
    health_checker: Arc<RwLock<HealthChecker>>,
) -> anyhow::Result<()> {
    let mgmt_state = MgmtState {
        orchestrator: state,
        health_checker,
    };

    let app = Router::new()
        .route("/api/status", get(get_status))
        .route("/api/goals", get(list_goals))
        .route("/api/goals", post(submit_goal))
        .route("/api/goals/:goal_id/tasks", get(get_goal_tasks))
        .route("/api/goals/:goal_id/messages", get(get_goal_messages))
        .route("/api/goals/:goal_id/messages", post(post_goal_message))
        .route("/api/chat", post(chat_handler))
        .route("/api/agents", get(list_agents))
        .route("/api/health", get(health_check))
        .route("/ws", get(ws_handler))
        .route("/", get(dashboard))
        .with_state(mgmt_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await?;
    info!("Management console listening on http://0.0.0.0:9090");

    axum::serve(listener, app).await?;
    Ok(())
}

// --- API Types ---

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    version: String,
    active_goals: usize,
    pending_tasks: usize,
    active_agents: usize,
    uptime_seconds: u64,
    autonomy_level: String,
}

#[derive(Serialize)]
struct GoalResponse {
    id: String,
    description: String,
    status: String,
    priority: i32,
    created_at: i64,
}

#[derive(Serialize)]
struct GoalTaskResponse {
    task_id: String,
    description: String,
    status: String,
    intelligence_level: String,
    output: String,
    model_used: String,
    error: String,
    created_at: i64,
    completed_at: i64,
}

#[derive(Deserialize)]
struct SubmitGoalRequest {
    description: String,
    #[serde(default = "default_priority")]
    priority: i32,
    #[serde(default)]
    provider: String,
}

fn default_priority() -> i32 {
    2
}

#[derive(Serialize)]
struct SubmitGoalResponse {
    goal_id: String,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    provider: String,
}

#[derive(Serialize)]
struct ChatResponse {
    reply: String,
    model: String,
    tokens: i32,
    latency_ms: i64,
}

#[derive(Serialize)]
struct AgentResponse {
    agent_id: String,
    agent_type: String,
    status: String,
    capabilities: Vec<String>,
}

#[derive(Deserialize)]
struct PostMessageRequest {
    content: String,
}

#[derive(Serialize)]
struct GoalMessageResponse {
    id: String,
    sender: String,
    content: String,
    timestamp: i64,
}

#[derive(Serialize)]
struct HealthResponse {
    healthy: bool,
    services: Vec<ServiceHealth>,
}

#[derive(Serialize)]
struct ServiceHealth {
    name: String,
    status: String,
    latency_ms: u64,
}

// --- Handlers ---

async fn get_status(State(state): State<MgmtState>) -> Json<StatusResponse> {
    let s = state.orchestrator.read().await;
    let uptime = s.started_at.elapsed().as_secs();
    Json(StatusResponse {
        status: "running".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        active_goals: s.goal_engine.active_goal_count(),
        pending_tasks: s.task_planner.pending_task_count(),
        active_agents: s.agent_router.active_agent_count(),
        uptime_seconds: uptime,
        autonomy_level: "full".into(),
    })
}

async fn list_goals(State(state): State<MgmtState>) -> Json<Vec<GoalResponse>> {
    let s = state.orchestrator.read().await;
    let (goals, _) = s.goal_engine.list_goals("", 50, 0).await;
    let response: Vec<GoalResponse> = goals
        .into_iter()
        .map(|g| GoalResponse {
            id: g.id,
            description: g.description,
            status: g.status,
            priority: g.priority,
            created_at: g.created_at,
        })
        .collect();
    Json(response)
}

/// Get tasks and their outputs for a specific goal
async fn get_goal_tasks(
    State(state): State<MgmtState>,
    Path(goal_id): Path<String>,
) -> Result<Json<Vec<GoalTaskResponse>>, StatusCode> {
    let s = state.orchestrator.read().await;
    match s.goal_engine.get_goal_with_tasks(&goal_id).await {
        Ok((_goal, tasks)) => {
            let response: Vec<GoalTaskResponse> = tasks
                .into_iter()
                .map(|t| {
                    let output_text = String::from_utf8_lossy(&t.output_json).to_string();
                    // Try to extract the actual AI response text from JSON
                    let display_output = extract_ai_response(&output_text);
                    GoalTaskResponse {
                        task_id: t.id,
                        description: t.description,
                        status: t.status,
                        intelligence_level: t.intelligence_level,
                        output: display_output,
                        model_used: extract_json_field(&output_text, "model_used"),
                        error: t.error,
                        created_at: t.created_at,
                        completed_at: t.completed_at,
                    }
                })
                .collect();
            Ok(Json(response))
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// Get messages for a goal's conversation thread
async fn get_goal_messages(
    State(state): State<MgmtState>,
    Path(goal_id): Path<String>,
) -> Json<Vec<GoalMessageResponse>> {
    let s = state.orchestrator.read().await;
    let messages = s.goal_engine.get_messages(&goal_id);
    let response: Vec<GoalMessageResponse> = messages
        .into_iter()
        .map(|m| GoalMessageResponse {
            id: m.id,
            sender: m.sender,
            content: m.content,
            timestamp: m.timestamp,
        })
        .collect();
    Json(response)
}

/// Post a user message to a goal and resume awaiting tasks
async fn post_goal_message(
    State(state): State<MgmtState>,
    Path(goal_id): Path<String>,
    Json(req): Json<PostMessageRequest>,
) -> Result<Json<GoalMessageResponse>, StatusCode> {
    let mut s = state.orchestrator.write().await;

    let msg_id = s.goal_engine.add_message(&goal_id, "user", &req.content);
    let timestamp = chrono::Utc::now().timestamp();

    // Find tasks in "awaiting_input" for this goal and resume them
    let awaiting_tasks: Vec<String> = s
        .task_planner
        .get_tasks_for_goal(&goal_id)
        .iter()
        .filter(|t| t.status == "awaiting_input")
        .map(|t| t.id.clone())
        .collect();

    for task_id in &awaiting_tasks {
        s.task_planner.resume_task(task_id);
        s.goal_engine.update_task_status(&goal_id, task_id, "pending");
    }

    if !awaiting_tasks.is_empty() {
        tracing::info!(
            "Resumed {} awaiting tasks for goal {goal_id}",
            awaiting_tasks.len()
        );
    }

    Ok(Json(GoalMessageResponse {
        id: msg_id,
        sender: "user".to_string(),
        content: req.content,
        timestamp,
    }))
}

/// Build a system context string with real state for the AI chat
async fn build_system_context(state: &MgmtState) -> String {
    let s = state.orchestrator.read().await;
    let health = state.health_checker.read().await;
    let health_status = health.get_all_status();
    let uptime_secs = s.started_at.elapsed().as_secs();
    let uptime_str = if uptime_secs >= 3600 {
        format!("{}h {}m", uptime_secs / 3600, (uptime_secs % 3600) / 60)
    } else {
        format!("{}m {}s", uptime_secs / 60, uptime_secs % 60)
    };

    // Gather all goals with their tasks
    let (all_goals, total_goals) = s.goal_engine.list_goals("", 100, 0).await;
    let active_goals = s.goal_engine.active_goal_count();
    let pending_tasks = s.task_planner.pending_task_count();
    let active_agents = s.agent_router.active_agent_count();
    let agents = s.agent_router.list_agents().await;

    let mut context = format!(
        r#"You are aiOS, an AI-native operating system where AI agents replace traditional system services. You ARE the operating system — you control init, scheduling, services, and all system operations autonomously.

## Current System State (LIVE DATA)
- **Uptime**: {uptime_str}
- **Active Goals**: {active_goals}
- **Pending Tasks**: {pending_tasks}
- **Total Goals Ever**: {total_goals}
- **Active Agents**: {active_agents}

## Services
"#
    );

    for svc in &health_status {
        let status = if svc.healthy { "HEALTHY" } else { "UNHEALTHY" };
        context.push_str(&format!("- {} — {} ({}ms latency)\n", svc.name, status, svc.last_check_ms));
    }

    // Add registered agents
    if !agents.is_empty() {
        context.push_str("\n## Registered Agents\n");
        for a in &agents {
            context.push_str(&format!("- {} (type: {}, status: {}, capabilities: {})\n",
                a.agent_id, a.agent_type, a.status,
                a.capabilities.join(", ")));
        }
    }

    // Add goal history with task details and AI outputs
    if !all_goals.is_empty() {
        context.push_str("\n## Goal History (all goals in memory)\n");
        for goal in &all_goals {
            context.push_str(&format!("\n### Goal [{}] — Status: {}, Priority: {}\n",
                &goal.id[..8.min(goal.id.len())], goal.status, goal.priority));
            context.push_str(&format!("Description: {}\n", goal.description));
            context.push_str(&format!("Source: {}, Created: {}\n", goal.source, goal.created_at));

            // Get tasks for this goal
            if let Ok((_g, tasks)) = s.goal_engine.get_goal_with_tasks(&goal.id).await {
                if !tasks.is_empty() {
                    context.push_str("Tasks:\n");
                    for task in &tasks {
                        context.push_str(&format!("  - [{}] {} — status: {}, level: {}\n",
                            &task.id[..8.min(task.id.len())], task.description, task.status, task.intelligence_level));
                        // Include AI output if available
                        let output = String::from_utf8_lossy(&task.output_json);
                        if !output.is_empty() {
                            let ai_text = extract_ai_response(&output);
                            if !ai_text.is_empty() && ai_text.len() > 2 {
                                // Truncate very long outputs to save tokens
                                let truncated = if ai_text.len() > 1500 {
                                    format!("{}... [truncated, {} chars total]", &ai_text[..1500], ai_text.len())
                                } else {
                                    ai_text
                                };
                                context.push_str(&format!("    AI Output: {}\n", truncated));
                            }
                        }
                        if !task.error.is_empty() {
                            context.push_str(&format!("    Error: {}\n", task.error));
                        }
                    }
                }
            }
        }
    } else {
        context.push_str("\n## Goal History\nNo goals have been submitted yet.\n");
    }

    context.push_str(r#"
## Your Capabilities
You can manage this system through goals and tasks. When the user submits a goal via the Goals tab, it gets decomposed into tasks and executed by the autonomy loop using AI inference. You have access to 40+ system tools (filesystem, process, network, service management, etc.) and 8 specialized agents (system, task, network, security, package, monitor, learning, meta).

## How to Respond
- When asked about goals/tasks: reference the ACTUAL data above, not hypothetical
- When asked about system state: use the LIVE metrics above
- When asked what you've done: describe the actual AI output from completed tasks
- Be specific and factual — you have real data, use it
- If the user asks you to do something, suggest they submit it as a goal in the Goals tab
"#);

    context
}

/// Chat endpoint — send a message directly to the AI and get a response
async fn chat_handler(
    State(state): State<MgmtState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    // Build rich system context with real state
    let system_prompt = build_system_context(&state).await;

    let s = state.orchestrator.read().await;

    // Try API gateway (Qwen3)
    match s.clients.api_gateway().await {
        Ok(mut client) => {
            let request = tonic::Request::new(crate::proto::api_gateway::ApiInferRequest {
                prompt: req.message.clone(),
                system_prompt,
                max_tokens: 4096,
                temperature: 0.7,
                preferred_provider: req.provider.clone(),
                requesting_agent: "chat-console".to_string(),
                task_id: String::new(),
                allow_fallback: true,
            });

            match client.infer(request).await {
                Ok(response) => {
                    let resp: crate::proto::common::InferenceResponse = response.into_inner();
                    Ok(Json(ChatResponse {
                        reply: resp.text,
                        model: resp.model_used,
                        tokens: resp.tokens_used,
                        latency_ms: resp.latency_ms,
                    }))
                }
                Err(e) => {
                    warn!("Chat inference failed: {e}");
                    Ok(Json(ChatResponse {
                        reply: format!("AI backend error: {e}. Make sure API gateway is running and configured."),
                        model: "error".into(),
                        tokens: 0,
                        latency_ms: 0,
                    }))
                }
            }
        }
        Err(e) => {
            warn!("Cannot connect to API gateway for chat: {e}");
            Ok(Json(ChatResponse {
                reply: format!("Cannot connect to AI backend: {e}"),
                model: "error".into(),
                tokens: 0,
                latency_ms: 0,
            }))
        }
    }
}

async fn submit_goal(
    State(state): State<MgmtState>,
    Json(req): Json<SubmitGoalRequest>,
) -> Result<Json<SubmitGoalResponse>, StatusCode> {
    let mut s = state.orchestrator.write().await;
    let description = req.description.clone();
    let provider = req.provider.clone();
    match s
        .goal_engine
        .submit_goal(req.description, req.priority, "management-console".into())
        .await
    {
        Ok(id) => {
            // Store preferred provider in goal metadata
            if !provider.is_empty() {
                let metadata = format!("{{\"preferred_provider\":\"{provider}\"}}");
                s.goal_engine.set_metadata(&id, metadata.into_bytes());
            }

            // Decompose goal into executable tasks so the autonomy loop can process them
            match s.task_planner.decompose_goal(&id, &description).await {
                Ok(tasks) => {
                    let task_count = tasks.len();
                    s.goal_engine.add_tasks(&id, tasks);
                    info!("Goal {id} decomposed into {task_count} tasks (provider: {provider})");
                }
                Err(e) => {
                    warn!("Failed to decompose goal {id}: {e}");
                }
            }
            Ok(Json(SubmitGoalResponse { goal_id: id }))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn list_agents(State(state): State<MgmtState>) -> Json<Vec<AgentResponse>> {
    let s = state.orchestrator.read().await;
    let agents = s.agent_router.list_agents().await;
    let response: Vec<AgentResponse> = agents
        .into_iter()
        .map(|a| AgentResponse {
            agent_id: a.agent_id,
            agent_type: a.agent_type,
            status: a.status,
            capabilities: a.capabilities,
        })
        .collect();
    Json(response)
}

async fn health_check(State(state): State<MgmtState>) -> Json<HealthResponse> {
    let checker = state.health_checker.read().await;
    let statuses = checker.get_all_status();

    let mut services = vec![ServiceHealth {
        name: "orchestrator".into(),
        status: "healthy".into(),
        latency_ms: 0,
    }];

    for svc in &statuses {
        services.push(ServiceHealth {
            name: svc.name.clone(),
            status: if svc.healthy {
                "healthy".into()
            } else {
                "unhealthy".into()
            },
            latency_ms: svc.last_check_ms,
        });
    }

    let healthy = statuses.iter().all(|s| s.healthy);

    Json(HealthResponse {
        healthy,
        services,
    })
}

/// WebSocket handler for real-time updates
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<MgmtState>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Handle a WebSocket connection — push status updates every 2 seconds
async fn handle_ws(mut socket: WebSocket, state: MgmtState) {
    info!("WebSocket client connected");

    loop {
        // Gather current status
        let update = {
            let s = state.orchestrator.read().await;
            let health = state.health_checker.read().await;
            let health_status = health.get_all_status();

            serde_json::json!({
                "type": "status_update",
                "active_goals": s.goal_engine.active_goal_count(),
                "pending_tasks": s.task_planner.pending_task_count(),
                "active_agents": s.agent_router.active_agent_count(),
                "uptime_seconds": s.started_at.elapsed().as_secs(),
                "services": health_status.iter().map(|h| {
                    serde_json::json!({
                        "name": h.name,
                        "healthy": h.healthy,
                        "latency_ms": h.last_check_ms,
                    })
                }).collect::<Vec<_>>(),
            })
        };

        if socket
            .send(Message::Text(update.to_string()))
            .await
            .is_err()
        {
            break;
        }

        // Check for client messages (ping/close)
        match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            socket.recv(),
        )
        .await
        {
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => break,
            Ok(Some(Err(_))) => break,
            _ => {} // Timeout or other message — continue
        }
    }

    info!("WebSocket client disconnected");
}

/// Extract AI response text from JSON output
fn extract_ai_response(output: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
        // Try ai_response field first
        if let Some(ai_resp) = parsed.get("ai_response").and_then(|v| v.as_str()) {
            // The AI response might itself be JSON, try to extract readable content
            if let Ok(inner) = serde_json::from_str::<serde_json::Value>(ai_resp) {
                if let Some(result) = inner.get("result").and_then(|v| v.as_str()) {
                    return result.to_string();
                }
                if let Some(reasoning) = inner.get("reasoning").and_then(|v| v.as_str()) {
                    let result = inner.get("result").and_then(|v| v.as_str()).unwrap_or("");
                    return format!("{}\n\n{}", reasoning, result);
                }
                return serde_json::to_string_pretty(&inner).unwrap_or_else(|_| ai_resp.to_string());
            }
            return ai_resp.to_string();
        }
        // Try result field
        if let Some(result) = parsed.get("result").and_then(|v| v.as_str()) {
            return result.to_string();
        }
        // Try reasoning + result
        if let Some(reasoning) = parsed.get("reasoning").and_then(|v| v.as_str()) {
            let result = parsed.get("result").and_then(|v| v.as_str()).unwrap_or("");
            return format!("{}\n\n{}", reasoning, result);
        }
        // Return prettified JSON
        return serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| output.to_string());
    }
    output.to_string()
}

/// Extract a string field from JSON
fn extract_json_field(json_str: &str, field: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json_str)
        .ok()
        .and_then(|v| v.get(field).and_then(|f| f.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

async fn dashboard() -> axum::response::Html<String> {
    axum::response::Html(DASHBOARD_HTML.to_string())
}

const DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html>
<head>
    <title>aiOS Management Console</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        * { box-sizing: border-box; }
        body { font-family: 'SF Mono', 'Fira Code', monospace; background: #0a0e1a; color: #e0e0e0; padding: 20px; margin: 0; }
        h1 { color: #00d4ff; margin-bottom: 4px; }
        h2 { color: #00d4ff; margin: 0 0 12px 0; font-size: 1.1em; }
        .card { background: #111827; border: 1px solid #1e3a5f; border-radius: 8px; padding: 16px; margin: 10px 0; }
        .metric { display: inline-block; margin: 0 20px 0 0; }
        .metric-value { font-size: 2em; color: #00d4ff; }
        .metric-label { font-size: 0.85em; color: #6b7280; }
        table { width: 100%; border-collapse: collapse; }
        th, td { padding: 8px 10px; text-align: left; border-bottom: 1px solid #1e3a5f; font-size: 0.9em; }
        th { color: #00d4ff; font-weight: 600; }
        button { background: #1e3a5f; color: #e0e0e0; border: 1px solid #00d4ff; padding: 8px 20px; cursor: pointer; border-radius: 4px; font-family: inherit; }
        button:hover { background: #00d4ff; color: #0a0e1a; }
        button:disabled { opacity: 0.5; cursor: not-allowed; }
        textarea, input { background: #111827; color: #e0e0e0; border: 1px solid #1e3a5f; padding: 10px; border-radius: 4px; width: 100%; font-family: inherit; font-size: 0.95em; resize: vertical; }
        textarea:focus, input:focus { outline: none; border-color: #00d4ff; }
        .ws-status { font-size: 0.7em; color: #6b7280; vertical-align: middle; }
        .ws-connected { color: #00ff88; }
        .svc-healthy { color: #00ff88; }
        .svc-unhealthy { color: #ff4444; }
        .status-completed { color: #00ff88; font-weight: bold; }
        .status-pending { color: #ffa500; }
        .status-in_progress { color: #00d4ff; }
        .status-failed { color: #ff4444; }
        .goal-row { cursor: pointer; }
        .goal-row:hover { background: #1e293b; }
        .output-box { background: #0d1117; border: 1px solid #1e3a5f; border-radius: 6px; padding: 14px; margin: 8px 0; white-space: pre-wrap; word-wrap: break-word; font-size: 0.9em; line-height: 1.5; max-height: 400px; overflow-y: auto; }
        .chat-container { display: flex; flex-direction: column; height: 500px; }
        .chat-messages { flex: 1; overflow-y: auto; padding: 10px; background: #0d1117; border: 1px solid #1e3a5f; border-radius: 6px 6px 0 0; }
        .chat-input-row { display: flex; gap: 8px; align-items: flex-end; }
        .chat-input-row textarea { border-radius: 0 0 0 6px; flex: 1; }
        .chat-input-row button { border-radius: 0 0 6px 0; min-width: 80px; }
        select { background: #111827; color: #e0e0e0; border: 1px solid #1e3a5f; padding: 8px 10px; border-radius: 4px; font-family: inherit; font-size: 0.85em; cursor: pointer; }
        select:focus { outline: none; border-color: #00d4ff; }
        .provider-bar { display: flex; align-items: center; gap: 12px; margin-bottom: 8px; font-size: 0.85em; }
        .provider-bar label { color: #6b7280; }
        .provider-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: 4px; }
        .provider-dot.available { background: #00ff88; }
        .provider-dot.unavailable { background: #ff4444; }
        .msg { margin: 8px 0; padding: 10px 14px; border-radius: 8px; line-height: 1.5; }
        .msg-user { background: #1e3a5f; margin-left: 40px; }
        .msg-ai { background: #1a2332; margin-right: 40px; border: 1px solid #1e3a5f; }
        .msg-label { font-size: 0.75em; color: #6b7280; margin-bottom: 4px; }
        .msg-meta { font-size: 0.75em; color: #4b5563; margin-top: 6px; }
        .msg-content { white-space: pre-wrap; word-wrap: break-word; }
        .spinner { display: inline-block; width: 16px; height: 16px; border: 2px solid #1e3a5f; border-top-color: #00d4ff; border-radius: 50%; animation: spin 0.8s linear infinite; vertical-align: middle; margin-right: 8px; }
        @keyframes spin { to { transform: rotate(360deg); } }
        .tabs { display: flex; gap: 0; margin-bottom: 0; }
        .tab { padding: 10px 24px; cursor: pointer; border: 1px solid #1e3a5f; border-bottom: none; border-radius: 8px 8px 0 0; background: #0d1117; color: #6b7280; }
        .tab.active { background: #111827; color: #00d4ff; border-color: #1e3a5f; }
        .tab-content { display: none; }
        .tab-content.active { display: block; }
        .grid-2 { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
        @media (max-width: 900px) { .grid-2 { grid-template-columns: 1fr; } }
    </style>
</head>
<body>
    <h1>aiOS Management Console <span class="ws-status" id="ws-status">connecting...</span></h1>
    <p style="color:#4b5563;margin-top:0">Autonomous AI Operating System</p>

    <div class="card">
        <div class="metric"><div class="metric-value" id="goals">-</div><div class="metric-label">Active Goals</div></div>
        <div class="metric"><div class="metric-value" id="tasks">-</div><div class="metric-label">Pending Tasks</div></div>
        <div class="metric"><div class="metric-value" id="agents">-</div><div class="metric-label">Active Agents</div></div>
        <div class="metric"><div class="metric-value" id="uptime">-</div><div class="metric-label">Uptime</div></div>
        <div class="metric"><div class="metric-value" style="color:#00ff88" id="sys-status">-</div><div class="metric-label">System Status</div></div>
    </div>

    <div class="tabs">
        <div class="tab active" onclick="switchTab('chat')">Chat</div>
        <div class="tab" onclick="switchTab('goals-tab')">Goals & Tasks</div>
        <div class="tab" onclick="switchTab('system')">System</div>
    </div>

    <!-- CHAT TAB -->
    <div class="card tab-content active" id="chat" style="border-radius: 0 8px 8px 8px">
        <div class="chat-container">
            <div class="provider-bar">
                <label>Model:</label>
                <select id="provider-select">
                    <option value="">Auto (best available)</option>
                    <option value="claude">Claude Sonnet 4</option>
                    <option value="openai">ChatGPT 5</option>
                    <option value="qwen3">Qwen3 30B</option>
                </select>
                <span id="provider-status" style="color:#6b7280;font-size:0.85em"></span>
            </div>
            <div class="chat-messages" id="chat-messages">
                <div class="msg msg-ai">
                    <div class="msg-label">aiOS</div>
                    <div class="msg-content">Hello! I'm aiOS, your AI operating system. Select a model above and ask me anything.</div>
                </div>
            </div>
            <div class="chat-input-row">
                <textarea id="chat-input" rows="2" placeholder="Ask aiOS anything..." onkeydown="if(event.key==='Enter'&&!event.shiftKey){event.preventDefault();sendChat()}"></textarea>
                <button id="chat-send-btn" onclick="sendChat()">Send</button>
            </div>
        </div>
    </div>

    <!-- GOALS TAB -->
    <div class="card tab-content" id="goals-tab" style="border-radius: 0 8px 8px 8px">
        <div class="grid-2">
            <div>
                <h2>Submit Goal</h2>
                <textarea id="goal-input" rows="2" placeholder="Describe what you want the system to do..."></textarea>
                <div class="provider-bar" style="margin-top:8px">
                    <label>Model:</label>
                    <select id="goal-provider-select">
                        <option value="">Auto (best available)</option>
                        <option value="claude">Claude Sonnet 4</option>
                        <option value="openai">ChatGPT 5</option>
                        <option value="qwen3">Qwen3 30B</option>
                    </select>
                </div>
                <button onclick="submitGoal()" id="goal-submit-btn">Submit Goal</button>
                <span id="goal-result" style="margin-left:10px;color:#6b7280"></span>
            </div>
            <div>
                <h2>Goal Chat</h2>
                <div id="goal-chat-area" style="min-height:300px;max-height:500px;overflow-y:auto;background:#0d1117;border:1px solid #1e3a5f;border-radius:6px;padding:10px">
                    <div style="color:#6b7280;text-align:center;padding:40px 0">Click on a goal to see its progress and chat...</div>
                </div>
                <div id="goal-reply-area" style="display:none;margin-top:8px">
                    <div style="background:#332200;border:1px solid #ffa500;border-radius:4px;padding:8px;margin-bottom:8px;font-size:0.85em;color:#ffa500">AI is awaiting your input</div>
                    <div class="chat-input-row">
                        <textarea id="goal-reply-input" rows="2" placeholder="Reply to AI..." onkeydown="if(event.key==='Enter'&&!event.shiftKey){event.preventDefault();sendGoalMessage()}"></textarea>
                        <button onclick="sendGoalMessage()">Reply</button>
                    </div>
                </div>
            </div>
        </div>
        <h2 style="margin-top:16px">Goals</h2>
        <table><thead><tr><th>ID</th><th>Description</th><th>Status</th><th>Priority</th></tr></thead>
        <tbody id="goals-table"></tbody></table>
    </div>

    <!-- SYSTEM TAB -->
    <div class="card tab-content" id="system" style="border-radius: 0 8px 8px 8px">
        <div class="grid-2">
            <div>
                <h2>Service Health</h2>
                <table><thead><tr><th>Service</th><th>Status</th><th>Latency</th></tr></thead>
                <tbody id="health-table"></tbody></table>
            </div>
            <div>
                <h2>Agents</h2>
                <table><thead><tr><th>ID</th><th>Type</th><th>Status</th><th>Capabilities</th></tr></thead>
                <tbody id="agents-table"></tbody></table>
            </div>
        </div>
    </div>

    <script>
        // --- Tabs ---
        function switchTab(tabId) {
            document.querySelectorAll('.tab-content').forEach(el => el.classList.remove('active'));
            document.querySelectorAll('.tab').forEach(el => el.classList.remove('active'));
            document.getElementById(tabId).classList.add('active');
            event.target.classList.add('active');
        }

        // --- WebSocket ---
        let ws;
        function connectWS() {
            const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
            ws = new WebSocket(`${proto}//${location.host}/ws`);
            ws.onopen = () => {
                document.getElementById('ws-status').textContent = 'live';
                document.getElementById('ws-status').className = 'ws-status ws-connected';
            };
            ws.onmessage = (event) => {
                const data = JSON.parse(event.data);
                if (data.type === 'status_update') {
                    document.getElementById('goals').textContent = data.active_goals;
                    document.getElementById('tasks').textContent = data.pending_tasks;
                    document.getElementById('agents').textContent = data.active_agents;
                    document.getElementById('sys-status').textContent = 'running';
                    const mins = Math.floor(data.uptime_seconds / 60);
                    const hrs = Math.floor(mins / 60);
                    document.getElementById('uptime').textContent = hrs > 0 ? `${hrs}h ${mins%60}m` : `${mins}m`;
                    if (data.services) {
                        document.getElementById('health-table').innerHTML = data.services.map(s =>
                            `<tr><td>${s.name}</td><td class="${s.healthy ? 'svc-healthy' : 'svc-unhealthy'}">${s.healthy ? 'healthy' : 'unhealthy'}</td><td>${s.latency_ms}ms</td></tr>`
                        ).join('');
                    }
                }
            };
            ws.onclose = () => {
                document.getElementById('ws-status').textContent = 'disconnected';
                document.getElementById('ws-status').className = 'ws-status';
                setTimeout(connectWS, 3000);
            };
        }
        connectWS();

        // --- Chat ---
        async function sendChat() {
            const input = document.getElementById('chat-input');
            const msg = input.value.trim();
            if (!msg) return;
            input.value = '';
            const btn = document.getElementById('chat-send-btn');
            btn.disabled = true;
            btn.textContent = '...';

            const provider = document.getElementById('provider-select').value;
            const providerLabel = provider ? document.getElementById('provider-select').selectedOptions[0].text : 'Auto';

            const msgBox = document.getElementById('chat-messages');
            msgBox.innerHTML += `<div class="msg msg-user"><div class="msg-label">You</div><div class="msg-content">${escapeHtml(msg)}</div></div>`;
            msgBox.innerHTML += `<div class="msg msg-ai" id="thinking"><div class="msg-label"><span class="spinner"></span>aiOS is thinking via ${escapeHtml(providerLabel)}...</div></div>`;
            msgBox.scrollTop = msgBox.scrollHeight;

            try {
                const res = await fetch('/api/chat', {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify({message: msg, provider: provider})
                });
                const data = await res.json();
                const thinkEl = document.getElementById('thinking');
                if (thinkEl) thinkEl.remove();
                msgBox.innerHTML += `<div class="msg msg-ai"><div class="msg-label">aiOS <span style="color:#6b7280;font-size:0.85em">(${escapeHtml(data.model)})</span></div><div class="msg-content">${formatResponse(data.reply)}</div><div class="msg-meta">${data.model} | ${data.tokens} tokens | ${(data.latency_ms/1000).toFixed(1)}s</div></div>`;
            } catch(e) {
                const thinkEl = document.getElementById('thinking');
                if (thinkEl) thinkEl.remove();
                msgBox.innerHTML += `<div class="msg msg-ai"><div class="msg-label">Error</div><div class="msg-content" style="color:#ff4444">${e}</div></div>`;
            }
            btn.disabled = false;
            btn.textContent = 'Send';
            msgBox.scrollTop = msgBox.scrollHeight;
        }

        function escapeHtml(text) {
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }

        function formatResponse(text) {
            // Basic markdown-ish formatting
            let html = escapeHtml(text);
            // Bold
            html = html.replace(/\*\*(.*?)\*\*/g, '<strong>$1</strong>');
            // Code blocks
            html = html.replace(/```(\w*)\n([\s\S]*?)```/g, '<pre style="background:#0a0e1a;padding:8px;border-radius:4px;overflow-x:auto">$2</pre>');
            // Inline code
            html = html.replace(/`([^`]+)`/g, '<code style="background:#0a0e1a;padding:2px 6px;border-radius:3px">$1</code>');
            return html;
        }

        // --- Goals ---
        let currentGoalId = null;
        let goalRefreshInterval = null;

        async function refresh() {
            try {
                const goals = await (await fetch('/api/goals')).json();
                document.getElementById('goals-table').innerHTML = goals.map(g =>
                    `<tr class="goal-row" onclick="loadGoalChat('${g.id}')"><td>${g.id.slice(0,8)}</td><td>${escapeHtml(g.description.slice(0,80))}${g.description.length>80?'...':''}</td><td class="status-${g.status}">${g.status}</td><td>${g.priority}</td></tr>`
                ).join('');

                const agents = await (await fetch('/api/agents')).json();
                document.getElementById('agents-table').innerHTML = agents.map(a =>
                    `<tr><td>${a.agent_id.slice(0,8)}</td><td>${a.agent_type}</td><td>${a.status}</td><td>${a.capabilities.slice(0,3).join(', ')}${a.capabilities.length>3?'...':''}</td></tr>`
                ).join('') || '<tr><td colspan="4" style="color:#6b7280">No agents registered</td></tr>';
            } catch(e) { console.error('Refresh failed:', e); }
        }

        async function loadGoalChat(goalId) {
            currentGoalId = goalId;
            const area = document.getElementById('goal-chat-area');
            area.innerHTML = '<div style="color:#6b7280;text-align:center;padding:20px"><span class="spinner"></span> Loading...</div>';
            try {
                const [messages, tasks] = await Promise.all([
                    fetch(`/api/goals/${goalId}/messages`).then(r => r.json()),
                    fetch(`/api/goals/${goalId}/tasks`).then(r => r.json()),
                ]);
                let items = [];
                for (const m of messages) { items.push({ type: 'message', ...m }); }
                for (const t of tasks) { items.push({ type: 'task', ...t, timestamp: t.completed_at || t.created_at }); }
                items.sort((a, b) => a.timestamp - b.timestamp);

                let html = '';
                for (const item of items) {
                    if (item.type === 'message') {
                        if (item.sender === 'system') {
                            html += `<div style="color:#6b7280;font-size:0.8em;padding:4px 8px;margin:4px 0">${escapeHtml(item.content)}</div>`;
                        } else if (item.sender === 'ai') {
                            html += `<div class="msg msg-ai" style="margin:6px 0"><div class="msg-label">AI</div><div class="msg-content">${formatResponse(item.content)}</div></div>`;
                        } else {
                            html += `<div class="msg msg-user" style="margin:6px 0"><div class="msg-label">You</div><div class="msg-content">${escapeHtml(item.content)}</div></div>`;
                        }
                    } else {
                        const sc = item.status === 'completed' ? '#00ff88' : item.status === 'failed' ? '#ff4444' : item.status === 'awaiting_input' ? '#ffa500' : item.status === 'in_progress' ? '#00d4ff' : '#6b7280';
                        const badge = `<span style="background:${sc}22;color:${sc};padding:2px 8px;border-radius:10px;font-size:0.8em">${item.status}</span>`;
                        html += `<div style="border:1px solid #1e3a5f;border-radius:6px;padding:10px;margin:8px 0">`;
                        html += `<div style="display:flex;justify-content:space-between;align-items:center"><span style="color:#00d4ff;font-weight:bold;font-size:0.9em">${escapeHtml(item.description.slice(0,60))}</span>${badge}</div>`;
                        if (item.output) {
                            html += `<div style="margin-top:6px;font-size:0.85em;color:#c0c0c0;white-space:pre-wrap;max-height:200px;overflow-y:auto">${formatResponse(item.output)}</div>`;
                        }
                        if (item.error) {
                            html += `<div style="color:#ff4444;font-size:0.85em;margin-top:4px">Error: ${escapeHtml(item.error)}</div>`;
                        }
                        html += `</div>`;
                    }
                }
                area.innerHTML = html || '<div style="color:#6b7280;text-align:center;padding:20px">No activity yet...</div>';
                area.scrollTop = area.scrollHeight;

                const hasAwaiting = tasks.some(t => t.status === 'awaiting_input');
                document.getElementById('goal-reply-area').style.display = hasAwaiting ? 'block' : 'none';
            } catch(e) {
                area.innerHTML = `<div style="color:#ff4444;padding:20px">Failed to load: ${e}</div>`;
            }

            if (goalRefreshInterval) clearInterval(goalRefreshInterval);
            goalRefreshInterval = setInterval(() => {
                if (currentGoalId) loadGoalChat(currentGoalId);
            }, 2000);
        }

        async function sendGoalMessage() {
            if (!currentGoalId) return;
            const input = document.getElementById('goal-reply-input');
            const msg = input.value.trim();
            if (!msg) return;
            input.value = '';
            try {
                await fetch(`/api/goals/${currentGoalId}/messages`, {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify({ content: msg })
                });
                loadGoalChat(currentGoalId);
            } catch(e) { console.error('Failed to send message:', e); }
        }

        async function submitGoal() {
            const desc = document.getElementById('goal-input').value;
            if (!desc) return;
            const provider = document.getElementById('goal-provider-select').value;
            const btn = document.getElementById('goal-submit-btn');
            btn.disabled = true;
            btn.textContent = 'Submitting...';
            try {
                const res = await fetch('/api/goals', {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify({description: desc, priority: 2, provider: provider})
                });
                const data = await res.json();
                document.getElementById('goal-result').textContent = `Created: ${data.goal_id.slice(0,8)}`;
                document.getElementById('goal-input').value = '';
                refresh();
                loadGoalChat(data.goal_id);
            } catch(e) { document.getElementById('goal-result').textContent = `Error: ${e}`; }
            btn.disabled = false;
            btn.textContent = 'Submit Goal';
        }

        refresh();
        setInterval(refresh, 5000);
    </script>
</body>
</html>"##;
