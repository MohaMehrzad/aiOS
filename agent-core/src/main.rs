//! aiOS Orchestrator — Goal engine, task planner, agent router
//!
//! The brain of aiOS: receives goals, decomposes them into tasks,
//! routes tasks to agents, and manages the overall autonomy loop.

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tracing::{error, info, warn};

mod goal_engine;
mod task_planner;
mod agent_router;
mod result_aggregator;
mod decision_logger;
mod management;
mod health;
mod clients;
mod autonomy;
mod context;
mod agent_spawner;
mod tls;
mod discovery;
mod proactive;

pub mod proto {
    pub mod common {
        tonic::include_proto!("aios.common");
    }
    pub mod orchestrator {
        tonic::include_proto!("aios.orchestrator");
    }
    pub mod agent {
        tonic::include_proto!("aios.agent");
    }
    pub mod runtime {
        tonic::include_proto!("aios.runtime");
    }
    pub mod tools {
        tonic::include_proto!("aios.tools");
    }
    pub mod memory {
        tonic::include_proto!("aios.memory");
    }
    pub mod api_gateway {
        tonic::include_proto!("aios.api_gateway");
    }
}

use proto::orchestrator::orchestrator_server::OrchestratorServer;

/// Shared orchestrator state
pub struct OrchestratorState {
    pub goal_engine: goal_engine::GoalEngine,
    pub task_planner: task_planner::TaskPlanner,
    pub agent_router: agent_router::AgentRouter,
    pub result_aggregator: result_aggregator::ResultAggregator,
    pub decision_logger: decision_logger::DecisionLogger,
    pub started_at: Instant,
    pub cancel_token: CancellationToken,
    pub clients: clients::ServiceClients,
    pub health_checker: Arc<RwLock<health::HealthChecker>>,
}

/// Read CPU usage from /proc/stat (Linux) or return 0.0 on other platforms
fn read_cpu_percent() -> f64 {
    #[cfg(target_os = "linux")]
    {
        // Read /proc/loadavg for 1-minute load average, normalize by CPU count
        if let Ok(contents) = std::fs::read_to_string("/proc/loadavg") {
            if let Some(load_str) = contents.split_whitespace().next() {
                if let Ok(load) = load_str.parse::<f64>() {
                    let cpus = std::thread::available_parallelism()
                        .map(|n| n.get() as f64)
                        .unwrap_or(1.0);
                    return (load / cpus * 100.0).min(100.0);
                }
            }
        }
        0.0
    }
    #[cfg(not(target_os = "linux"))]
    {
        // On macOS/other, use available_parallelism as a rough proxy
        0.0
    }
}

/// Read memory info from /proc/meminfo (Linux) or return (0, 0) on other platforms
fn read_memory_mb() -> (f64, f64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/meminfo") {
            let mut total_kb: u64 = 0;
            let mut available_kb: u64 = 0;
            for line in contents.lines() {
                if line.starts_with("MemTotal:") {
                    total_kb = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                } else if line.starts_with("MemAvailable:") {
                    available_kb = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
                if total_kb > 0 && available_kb > 0 {
                    break;
                }
            }
            let total_mb = total_kb as f64 / 1024.0;
            let used_mb = (total_kb.saturating_sub(available_kb)) as f64 / 1024.0;
            return (used_mb, total_mb);
        }
        (0.0, 0.0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        (0.0, 0.0)
    }
}

/// gRPC service implementation
pub struct OrchestratorService {
    state: Arc<RwLock<OrchestratorState>>,
}

#[tonic::async_trait]
impl proto::orchestrator::orchestrator_server::Orchestrator for OrchestratorService {
    async fn submit_goal(
        &self,
        request: tonic::Request<proto::orchestrator::SubmitGoalRequest>,
    ) -> Result<tonic::Response<proto::common::GoalId>, tonic::Status> {
        let req = request.into_inner();
        info!("Received goal: {}", req.description);

        let mut state = self.state.write().await;

        // Decompose goal into tasks
        let goal_id = state
            .goal_engine
            .submit_goal(req.description.clone(), req.priority, req.source)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to submit goal: {e}")))?;

        // Decompose into tasks using the task planner
        match state
            .task_planner
            .decompose_goal(&goal_id, &req.description)
            .await
        {
            Ok(tasks) => {
                let task_count = tasks.len();
                state.goal_engine.add_tasks(&goal_id, tasks);
                info!("Goal {goal_id} decomposed into {task_count} tasks");
            }
            Err(e) => {
                warn!("Failed to decompose goal {goal_id}: {e}");
            }
        }

        Ok(tonic::Response::new(proto::common::GoalId { id: goal_id }))
    }

    async fn get_goal_status(
        &self,
        request: tonic::Request<proto::common::GoalId>,
    ) -> Result<tonic::Response<proto::orchestrator::GoalStatusResponse>, tonic::Status> {
        let goal_id = request.into_inner().id;
        let state = self.state.read().await;

        let (goal, tasks) = state
            .goal_engine
            .get_goal_with_tasks(&goal_id)
            .await
            .map_err(|e| tonic::Status::not_found(format!("Goal not found: {e}")))?;

        let progress = state.goal_engine.calculate_progress(&goal_id).await;

        Ok(tonic::Response::new(
            proto::orchestrator::GoalStatusResponse {
                goal: Some(goal),
                tasks,
                current_phase: "executing".to_string(),
                progress_percent: progress,
            },
        ))
    }

    async fn cancel_goal(
        &self,
        request: tonic::Request<proto::common::GoalId>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let goal_id = request.into_inner().id;
        let mut state = self.state.write().await;

        state
            .goal_engine
            .cancel_goal(&goal_id)
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to cancel goal: {e}")))?;

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: format!("Goal {goal_id} cancelled"),
        }))
    }

    async fn list_goals(
        &self,
        request: tonic::Request<proto::orchestrator::ListGoalsRequest>,
    ) -> Result<tonic::Response<proto::orchestrator::GoalListResponse>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;

        let (goals, total) = state
            .goal_engine
            .list_goals(&req.status_filter, req.limit, req.offset)
            .await;

        Ok(tonic::Response::new(
            proto::orchestrator::GoalListResponse { goals, total },
        ))
    }

    async fn register_agent(
        &self,
        request: tonic::Request<proto::common::AgentRegistration>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let registration = request.into_inner();
        info!(
            "Agent registering: {} (type: {})",
            registration.agent_id, registration.agent_type
        );

        let mut state = self.state.write().await;
        state.agent_router.register_agent(registration).await;

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: "Agent registered".to_string(),
        }))
    }

    async fn unregister_agent(
        &self,
        request: tonic::Request<proto::common::AgentId>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let agent_id = request.into_inner().id;
        let mut state = self.state.write().await;
        state.agent_router.unregister_agent(&agent_id).await;

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: format!("Agent {agent_id} unregistered"),
        }))
    }

    async fn heartbeat(
        &self,
        request: tonic::Request<proto::orchestrator::HeartbeatRequest>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let hb = request.into_inner();
        let mut state = self.state.write().await;
        state.agent_router.update_heartbeat(&hb.agent_id, &hb.status);

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: "OK".to_string(),
        }))
    }

    async fn list_agents(
        &self,
        _request: tonic::Request<proto::common::Empty>,
    ) -> Result<tonic::Response<proto::orchestrator::AgentListResponse>, tonic::Status> {
        let state = self.state.read().await;
        let agents = state.agent_router.list_agents().await;

        Ok(tonic::Response::new(
            proto::orchestrator::AgentListResponse { agents },
        ))
    }

    async fn get_system_status(
        &self,
        _request: tonic::Request<proto::common::Empty>,
    ) -> Result<tonic::Response<proto::orchestrator::SystemStatusResponse>, tonic::Status> {
        let state = self.state.read().await;
        let uptime = state.started_at.elapsed().as_secs() as i64;
        let cpu = read_cpu_percent();
        let (mem_used, mem_total) = read_memory_mb();

        // Collect registered agent capabilities as a proxy for "loaded models"
        let agents = state.agent_router.list_agents().await;
        let mut models: Vec<String> = agents
            .iter()
            .flat_map(|a| a.capabilities.iter().cloned())
            .collect();
        models.sort();
        models.dedup();

        let status = proto::orchestrator::SystemStatusResponse {
            active_goals: state.goal_engine.active_goal_count() as i32,
            pending_tasks: state.task_planner.pending_task_count() as i32,
            active_agents: state.agent_router.active_agent_count() as i32,
            loaded_models: models,
            cpu_percent: cpu,
            memory_used_mb: mem_used,
            memory_total_mb: mem_total,
            autonomy_level: "full".to_string(),
            uptime_seconds: uptime,
        };

        Ok(tonic::Response::new(status))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .compact()
        .init();

    info!("aiOS Orchestrator starting...");

    // Create cancellation token for graceful shutdown
    let cancel_token = CancellationToken::new();

    // Initialize health checker
    let health_checker = Arc::new(RwLock::new(health::HealthChecker::new()));

    // Initialize service discovery
    let service_registry = Arc::new(RwLock::new(discovery::ServiceRegistry::new()));
    service_registry.write().await.register_defaults();

    // Initialize state with persistent goal storage
    let db_path = "/var/lib/aios/data/goals.db";
    let goal_eng = match goal_engine::GoalEngine::with_db(db_path) {
        Ok(engine) => engine,
        Err(e) => {
            tracing::warn!("Failed to open goals database at {db_path}: {e}, falling back to in-memory");
            goal_engine::GoalEngine::new()
        }
    };
    let state = Arc::new(RwLock::new(OrchestratorState {
        goal_engine: goal_eng,
        task_planner: task_planner::TaskPlanner::new(),
        agent_router: agent_router::AgentRouter::new(),
        result_aggregator: result_aggregator::ResultAggregator::new(),
        decision_logger: decision_logger::DecisionLogger::new(),
        started_at: Instant::now(),
        cancel_token: cancel_token.clone(),
        clients: clients::ServiceClients::new(),
        health_checker: health_checker.clone(),
    }));

    let service = OrchestratorService {
        state: state.clone(),
    };

    // Start management console (HTTP) in background
    let mgmt_state = state.clone();
    let mgmt_health = health_checker.clone();
    tokio::spawn(async move {
        if let Err(e) = management::start_management_server(mgmt_state, mgmt_health).await {
            error!("Management server failed: {e}");
        }
    });

    // Start health checker background loop
    let health_cancel = cancel_token.clone();
    let health_checker_clone = health_checker.clone();
    tokio::spawn(async move {
        health::HealthChecker::run(health_checker_clone, health_cancel).await;
    });

    // Start autonomy loop
    let autonomy_state = state.clone();
    let autonomy_cancel = cancel_token.clone();
    tokio::spawn(async move {
        autonomy::run_autonomy_loop(
            autonomy_state,
            autonomy_cancel,
            autonomy::AutonomyConfig::default(),
        )
        .await;
    });

    // Start proactive goal generator
    let proactive_state = state.clone();
    let proactive_cancel = cancel_token.clone();
    tokio::spawn(async move {
        proactive::run_proactive_loop(
            proactive_state,
            proactive_cancel,
            proactive::ProactiveConfig::default(),
        )
        .await;
    });

    // Start service discovery background loop
    let discovery_cancel = cancel_token.clone();
    tokio::spawn(async move {
        discovery::ServiceRegistry::run(service_registry, discovery_cancel).await;
    });

    // Set up signal handlers for graceful shutdown
    let shutdown_token = cancel_token.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let ctrl_c = tokio::signal::ctrl_c();
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("Failed to install SIGTERM handler");

            tokio::select! {
                _ = ctrl_c => {
                    info!("Received SIGINT, initiating graceful shutdown...");
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, initiating graceful shutdown...");
                }
            }
        }

        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
            info!("Received SIGINT, initiating graceful shutdown...");
        }

        // Signal all background tasks to stop
        shutdown_token.cancel();

        // Give background tasks time to drain
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        info!("Graceful shutdown complete");
    });

    // Start gRPC server
    let addr: SocketAddr = "0.0.0.0:50051".parse()?;
    info!("Orchestrator gRPC server listening on {addr}");

    Server::builder()
        .add_service(OrchestratorServer::new(service))
        .serve_with_shutdown(addr, cancel_token.cancelled_owned())
        .await
        .context("gRPC server failed")?;

    Ok(())
}
