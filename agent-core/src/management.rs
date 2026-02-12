//! Management Console — REST API and dashboard
//!
//! Provides HTTP endpoints for monitoring and controlling aiOS.
//! Runs on port 9090 alongside the gRPC server.

use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::OrchestratorState;

type SharedState = Arc<RwLock<OrchestratorState>>;

/// Start the management HTTP server on port 9090
pub async fn start_management_server(state: SharedState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/status", get(get_status))
        .route("/api/goals", get(list_goals))
        .route("/api/goals", post(submit_goal))
        .route("/api/agents", get(list_agents))
        .route("/api/health", get(health_check))
        .route("/", get(dashboard))
        .with_state(state);

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

#[derive(Deserialize)]
struct SubmitGoalRequest {
    description: String,
    #[serde(default = "default_priority")]
    priority: i32,
}

fn default_priority() -> i32 {
    2
}

#[derive(Serialize)]
struct SubmitGoalResponse {
    goal_id: String,
}

#[derive(Serialize)]
struct AgentResponse {
    agent_id: String,
    agent_type: String,
    status: String,
    capabilities: Vec<String>,
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
}

// --- Handlers ---

async fn get_status(State(state): State<SharedState>) -> Json<StatusResponse> {
    let s = state.read().await;
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

async fn list_goals(State(state): State<SharedState>) -> Json<Vec<GoalResponse>> {
    let s = state.read().await;
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

async fn submit_goal(
    State(state): State<SharedState>,
    Json(req): Json<SubmitGoalRequest>,
) -> Result<Json<SubmitGoalResponse>, StatusCode> {
    let mut s = state.write().await;
    match s
        .goal_engine
        .submit_goal(req.description, req.priority, "management-console".into())
        .await
    {
        Ok(id) => Ok(Json(SubmitGoalResponse { goal_id: id })),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn list_agents(State(state): State<SharedState>) -> Json<Vec<AgentResponse>> {
    let s = state.read().await;
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

async fn health_check(State(_state): State<SharedState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        healthy: true,
        services: vec![
            ServiceHealth {
                name: "orchestrator".into(),
                status: "healthy".into(),
            },
            ServiceHealth {
                name: "goal_engine".into(),
                status: "healthy".into(),
            },
        ],
    })
}

async fn dashboard() -> axum::response::Html<String> {
    axum::response::Html(DASHBOARD_HTML.to_string())
}

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>aiOS Management Console</title>
    <meta charset="utf-8">
    <style>
        body { font-family: monospace; background: #1a1a2e; color: #e0e0e0; padding: 20px; }
        h1 { color: #00d4ff; }
        .card { background: #16213e; border: 1px solid #0f3460; border-radius: 8px; padding: 16px; margin: 8px 0; }
        .status { color: #00ff88; }
        .metric { display: inline-block; margin: 0 16px; }
        .metric-value { font-size: 2em; color: #00d4ff; }
        .metric-label { font-size: 0.9em; color: #888; }
        table { width: 100%; border-collapse: collapse; }
        th, td { padding: 8px; text-align: left; border-bottom: 1px solid #0f3460; }
        th { color: #00d4ff; }
        button { background: #0f3460; color: #e0e0e0; border: 1px solid #00d4ff; padding: 8px 16px; cursor: pointer; border-radius: 4px; }
        button:hover { background: #00d4ff; color: #1a1a2e; }
        input, textarea { background: #16213e; color: #e0e0e0; border: 1px solid #0f3460; padding: 8px; border-radius: 4px; width: 100%; }
    </style>
</head>
<body>
    <h1>aiOS Management Console</h1>
    <div class="card">
        <div class="metric"><div class="metric-value" id="goals">-</div><div class="metric-label">Active Goals</div></div>
        <div class="metric"><div class="metric-value" id="tasks">-</div><div class="metric-label">Pending Tasks</div></div>
        <div class="metric"><div class="metric-value" id="agents">-</div><div class="metric-label">Active Agents</div></div>
        <div class="metric"><div class="metric-value status" id="status">-</div><div class="metric-label">System Status</div></div>
    </div>

    <div class="card">
        <h2>Submit Goal</h2>
        <textarea id="goal-input" rows="2" placeholder="Describe what you want the system to do..."></textarea>
        <br><br>
        <button onclick="submitGoal()">Submit Goal</button>
        <span id="goal-result"></span>
    </div>

    <div class="card">
        <h2>Goals</h2>
        <table><thead><tr><th>ID</th><th>Description</th><th>Status</th><th>Priority</th></tr></thead>
        <tbody id="goals-table"></tbody></table>
    </div>

    <div class="card">
        <h2>Agents</h2>
        <table><thead><tr><th>ID</th><th>Type</th><th>Status</th><th>Capabilities</th></tr></thead>
        <tbody id="agents-table"></tbody></table>
    </div>

    <script>
        async function refresh() {
            try {
                const status = await (await fetch('/api/status')).json();
                document.getElementById('goals').textContent = status.active_goals;
                document.getElementById('tasks').textContent = status.pending_tasks;
                document.getElementById('agents').textContent = status.active_agents;
                document.getElementById('status').textContent = status.status;

                const goals = await (await fetch('/api/goals')).json();
                document.getElementById('goals-table').innerHTML = goals.map(g =>
                    `<tr><td>${g.id.slice(0,8)}</td><td>${g.description}</td><td>${g.status}</td><td>${g.priority}</td></tr>`
                ).join('');

                const agents = await (await fetch('/api/agents')).json();
                document.getElementById('agents-table').innerHTML = agents.map(a =>
                    `<tr><td>${a.agent_id}</td><td>${a.agent_type}</td><td>${a.status}</td><td>${a.capabilities.join(', ')}</td></tr>`
                ).join('');
            } catch(e) { console.error('Refresh failed:', e); }
        }

        async function submitGoal() {
            const desc = document.getElementById('goal-input').value;
            if (!desc) return;
            try {
                const res = await fetch('/api/goals', {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify({description: desc, priority: 2})
                });
                const data = await res.json();
                document.getElementById('goal-result').textContent = `Goal created: ${data.goal_id}`;
                document.getElementById('goal-input').value = '';
                refresh();
            } catch(e) { document.getElementById('goal-result').textContent = `Error: ${e}`; }
        }

        refresh();
        setInterval(refresh, 5000);
    </script>
</body>
</html>"#;
