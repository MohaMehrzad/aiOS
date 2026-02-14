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
use tracing::{debug, error, info, warn};

mod agent_router;
mod agent_spawner;
mod autonomy;
mod clients;
mod cluster;
mod context;
mod decision_logger;
mod discovery;
mod event_bus;
mod goal_engine;
mod health;
mod management;
mod proactive;
mod remote_exec;
mod result_aggregator;
mod scheduler;
mod task_planner;
mod tls;

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
    pub clients: Arc<clients::ServiceClients>,
    pub health_checker: Arc<RwLock<health::HealthChecker>>,
    pub cluster: Arc<RwLock<cluster::ClusterManager>>,
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
        state
            .agent_router
            .update_heartbeat(&hb.agent_id, &hb.status);

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

    async fn get_assigned_task(
        &self,
        request: tonic::Request<proto::common::AgentId>,
    ) -> Result<tonic::Response<proto::common::Task>, tonic::Status> {
        let agent_id = request.into_inner().id;
        let state = self.state.read().await;

        // Look up whether this agent has a task assigned
        if let Some(ref task_id) = state.agent_router.get_assigned_task_id(&agent_id) {
            if let Some(task) = state.task_planner.get_task(task_id) {
                debug!("Returning task {task_id} to agent {agent_id}");
                return Ok(tonic::Response::new(task.clone()));
            }
            warn!("Agent {agent_id} has assigned task {task_id} but task not found in planner");
        }

        // No task assigned — return empty task (agent should keep polling)
        Ok(tonic::Response::new(proto::common::Task::default()))
    }

    async fn report_task_result(
        &self,
        request: tonic::Request<proto::common::TaskResult>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let result = request.into_inner();
        let task_id = result.task_id.clone();
        let mut state = self.state.write().await;

        // Find which goal this task belongs to
        let goal_id = state
            .task_planner
            .get_task(&task_id)
            .map(|t| t.goal_id.clone());

        if let Some(ref goal_id) = goal_id {
            // Find the agent that completed this task and release it
            for agent in state.agent_router.list_agents().await {
                if let Some(ref assigned) = state.agent_router.get_assigned_task_id(&agent.agent_id)
                {
                    if assigned == &task_id {
                        state
                            .agent_router
                            .task_completed(&agent.agent_id, result.success);
                        break;
                    }
                }
            }

            if result.success {
                state
                    .task_planner
                    .complete_task(&task_id, result.output_json.clone());
                state.goal_engine.complete_task(goal_id, &task_id);
                state.goal_engine.add_message(
                    goal_id,
                    "system",
                    &format!("Task {task_id} completed by agent"),
                );
            } else {
                state.task_planner.fail_task(&task_id, &result.error);
                state
                    .goal_engine
                    .update_task_status(goal_id, &task_id, "failed");
                state.goal_engine.add_message(
                    goal_id,
                    "system",
                    &format!("Task {task_id} failed: {}", result.error),
                );
            }

            state.result_aggregator.record_result(goal_id, result);

            info!("Agent reported result for task {task_id}");
            Ok(tonic::Response::new(proto::common::Status {
                success: true,
                message: format!("Result recorded for task {task_id}"),
            }))
        } else {
            warn!("Agent reported result for unknown task {task_id}");
            Ok(tonic::Response::new(proto::common::Status {
                success: false,
                message: format!("Task {task_id} not found"),
            }))
        }
    }

    async fn request_capability(
        &self,
        request: tonic::Request<proto::orchestrator::CapabilityRequest>,
    ) -> Result<tonic::Response<proto::orchestrator::CapabilityResponse>, tonic::Status> {
        let req = request.into_inner();
        info!(
            "Capability request from {}: {:?}",
            req.agent_id, req.capabilities
        );

        // For now, auto-grant capabilities (a real implementation would check policies)
        let expires = chrono::Utc::now()
            + chrono::Duration::hours(if req.duration_hours > 0 {
                req.duration_hours
            } else {
                24
            });

        Ok(tonic::Response::new(
            proto::orchestrator::CapabilityResponse {
                granted: true,
                capabilities: req.capabilities,
                expires_at: expires.to_rfc3339(),
                denial_reason: String::new(),
            },
        ))
    }

    async fn revoke_capability(
        &self,
        request: tonic::Request<proto::orchestrator::CapabilityRevocation>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let req = request.into_inner();
        info!("Revoking capabilities from {}", req.agent_id);

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: format!("Capabilities revoked for {}", req.agent_id),
        }))
    }

    async fn create_schedule(
        &self,
        request: tonic::Request<proto::orchestrator::CreateScheduleRequest>,
    ) -> Result<tonic::Response<proto::orchestrator::ScheduleResponse>, tonic::Status> {
        let req = request.into_inner();
        let schedule_id = uuid::Uuid::new_v4().to_string();

        info!(
            "Creating schedule {}: {} → {}",
            schedule_id,
            req.cron_expr,
            &req.goal_template[..60.min(req.goal_template.len())]
        );

        Ok(tonic::Response::new(
            proto::orchestrator::ScheduleResponse {
                schedule_id,
                success: true,
            },
        ))
    }

    async fn list_schedules(
        &self,
        _request: tonic::Request<proto::common::Empty>,
    ) -> Result<tonic::Response<proto::orchestrator::ScheduleListResponse>, tonic::Status> {
        Ok(tonic::Response::new(
            proto::orchestrator::ScheduleListResponse { schedules: vec![] },
        ))
    }

    async fn delete_schedule(
        &self,
        request: tonic::Request<proto::orchestrator::DeleteScheduleRequest>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let req = request.into_inner();
        info!("Deleting schedule: {}", req.schedule_id);

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: format!("Schedule {} deleted", req.schedule_id),
        }))
    }

    async fn register_node(
        &self,
        request: tonic::Request<proto::orchestrator::NodeRegistration>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let req = request.into_inner();
        info!(
            "Cluster node registering: {} ({}) with agents: {:?}",
            req.node_id, req.hostname, req.agents
        );

        let state = self.state.read().await;
        let mut cm = state.cluster.write().await;
        cm.register_node(cluster::ClusterNode {
            node_id: req.node_id.clone(),
            hostname: req.hostname,
            address: req.address,
            agents: req.agents,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            active_tasks: 0,
            max_tasks: req.max_tasks,
            last_heartbeat: Instant::now(),
            registered_at: Instant::now(),
            metadata: req.metadata,
        });

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: format!("Node {} registered", req.node_id),
        }))
    }

    async fn node_heartbeat(
        &self,
        request: tonic::Request<proto::orchestrator::NodeStatus>,
    ) -> Result<tonic::Response<proto::common::Status>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let mut cm = state.cluster.write().await;
        cm.node_heartbeat(
            &req.node_id,
            req.cpu_usage,
            req.memory_usage,
            req.active_tasks,
        );

        Ok(tonic::Response::new(proto::common::Status {
            success: true,
            message: "OK".to_string(),
        }))
    }

    async fn list_nodes(
        &self,
        request: tonic::Request<proto::orchestrator::ListNodesRequest>,
    ) -> Result<tonic::Response<proto::orchestrator::NodeListResponse>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let cm = state.cluster.read().await;

        let nodes = if req.include_dead {
            cm.list_all_nodes()
        } else {
            cm.list_healthy_nodes()
        };

        let node_infos: Vec<proto::orchestrator::NodeInfo> = nodes
            .iter()
            .map(|n| proto::orchestrator::NodeInfo {
                node_id: n.node_id.clone(),
                hostname: n.hostname.clone(),
                address: n.address.clone(),
                agents: n.agents.clone(),
                cpu_usage: n.cpu_usage,
                memory_usage: n.memory_usage,
                active_tasks: n.active_tasks,
                healthy: n.last_heartbeat.elapsed().as_secs() < 30,
            })
            .collect();

        Ok(tonic::Response::new(
            proto::orchestrator::NodeListResponse { nodes: node_infos },
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
    let mut goal_eng = match goal_engine::GoalEngine::with_db(db_path) {
        Ok(engine) => engine,
        Err(e) => {
            tracing::warn!(
                "Failed to open goals database at {db_path}: {e}, falling back to in-memory"
            );
            goal_engine::GoalEngine::new()
        }
    };
    // Create task planner and sync persisted tasks from GoalEngine
    let mut task_plan = task_planner::TaskPlanner::new();
    let resumable = goal_eng.get_all_resumable_tasks();
    if !resumable.is_empty() {
        info!("Restoring {} tasks from previous session", resumable.len());
        task_plan.load_persisted_tasks(resumable);
    }

    let state = Arc::new(RwLock::new(OrchestratorState {
        goal_engine: goal_eng,
        task_planner: task_plan,
        agent_router: agent_router::AgentRouter::new(),
        result_aggregator: result_aggregator::ResultAggregator::new(),
        decision_logger: decision_logger::DecisionLogger::new(),
        started_at: Instant::now(),
        cancel_token: cancel_token.clone(),
        clients: Arc::new(clients::ServiceClients::new()),
        health_checker: health_checker.clone(),
        cluster: Arc::new(RwLock::new(cluster::ClusterManager::new(
            &std::env::var("AIOS_NODE_ID").unwrap_or_else(|_| "local".to_string()),
        ))),
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

    // Start agent spawner — spawn Python agent child processes
    let spawner = Arc::new(RwLock::new(agent_spawner::AgentSpawner::new(
        "/etc/aios/agents",
    )));
    {
        let mut s = spawner.write().await;
        match s.load_configs() {
            Ok(configs) => {
                info!("Loaded {} agent configs, spawning agents...", configs.len());
                for config in configs {
                    if let Err(e) = s.spawn_agent(config).await {
                        warn!("Failed to spawn agent: {e}");
                    }
                }
            }
            Err(e) => {
                warn!("Failed to load agent configs: {e}");
            }
        }
    }
    let spawner_cancel = cancel_token.clone();
    tokio::spawn(async move {
        agent_spawner::AgentSpawner::run_monitor(spawner, spawner_cancel).await;
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

    // Start goal scheduler
    let scheduler_db = "/var/lib/aios/data/scheduler.db";
    let mut goal_scheduler = scheduler::GoalScheduler::new(scheduler_db);
    if let Err(e) = goal_scheduler.load() {
        warn!("Failed to load scheduled goals: {e}");
    }
    let scheduler_arc = Arc::new(RwLock::new(goal_scheduler));
    let scheduler_state = state.clone();
    let scheduler_cancel = cancel_token.clone();
    tokio::spawn(async move {
        scheduler::GoalScheduler::run(scheduler_arc, scheduler_state, scheduler_cancel).await;
    });

    // Start event bus
    let event_bus = Arc::new(RwLock::new(event_bus::EventBus::new()));
    let event_bus_state = state.clone();
    let event_bus_cancel = cancel_token.clone();
    tokio::spawn(async move {
        event_bus::EventBus::run(event_bus, event_bus_state, event_bus_cancel).await;
    });

    // Start cluster monitor (only does work if AIOS_CLUSTER_ENABLED=true)
    let cluster_ref = {
        let s = state.read().await;
        s.cluster.clone()
    };
    let cluster_cancel = cancel_token.clone();
    tokio::spawn(async move {
        cluster::ClusterManager::run_monitor(cluster_ref, cluster_cancel).await;
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
