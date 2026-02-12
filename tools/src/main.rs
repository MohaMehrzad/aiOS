//! aiOS Tool Registry — system tool registration and execution
//!
//! Provides a gRPC service for discovering, executing, and managing
//! system tools. All tool calls go through the execution pipeline:
//! validate → check permissions → backup → execute → audit.

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tracing::info;

mod registry;
mod executor;
mod audit;
mod backup;
mod schema;
pub mod fs;
pub mod process;
pub mod service;
pub mod net;
pub mod firewall;
pub mod pkg;
pub mod sec;
pub mod monitor;
pub mod hw;

pub mod proto {
    pub mod common {
        tonic::include_proto!("aios.common");
    }
    pub mod tools {
        tonic::include_proto!("aios.tools");
    }
}

use proto::tools::tool_registry_server::{ToolRegistry, ToolRegistryServer};

/// Shared tool registry state
pub struct ToolRegistryState {
    pub registry: registry::Registry,
    pub executor: executor::Executor,
    pub audit_log: audit::AuditLog,
    pub backup_manager: backup::BackupManager,
}

/// gRPC service implementation
pub struct ToolRegistryService {
    state: Arc<Mutex<ToolRegistryState>>,
}

#[tonic::async_trait]
impl ToolRegistry for ToolRegistryService {
    async fn list_tools(
        &self,
        request: tonic::Request<proto::tools::ListToolsRequest>,
    ) -> Result<tonic::Response<proto::tools::ListToolsResponse>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.lock().await;
        let tools = state.registry.list_tools(&req.namespace);

        Ok(tonic::Response::new(proto::tools::ListToolsResponse {
            tools,
        }))
    }

    async fn get_tool(
        &self,
        request: tonic::Request<proto::tools::GetToolRequest>,
    ) -> Result<tonic::Response<proto::tools::ToolDefinition>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.lock().await;

        state
            .registry
            .get_tool(&req.name)
            .ok_or_else(|| tonic::Status::not_found(format!("Tool not found: {}", req.name)))
            .map(tonic::Response::new)
    }

    async fn execute(
        &self,
        request: tonic::Request<proto::tools::ExecuteRequest>,
    ) -> Result<tonic::Response<proto::tools::ExecuteResponse>, tonic::Status> {
        let req = request.into_inner();
        info!(
            "Executing tool: {} (agent: {}, reason: {})",
            req.tool_name, req.agent_id, req.reason
        );

        let mut state = self.state.lock().await;

        // Destructure to avoid simultaneous borrow conflicts
        let ToolRegistryState {
            ref registry,
            ref executor,
            ref mut audit_log,
            ref mut backup_manager,
        } = *state;

        // Execute through the pipeline
        let response = executor
            .execute(
                registry,
                audit_log,
                backup_manager,
                req,
            )
            .await
            .map_err(|e| tonic::Status::internal(format!("Execution failed: {e}")))?;

        Ok(tonic::Response::new(response))
    }

    async fn rollback(
        &self,
        request: tonic::Request<proto::tools::RollbackRequest>,
    ) -> Result<tonic::Response<proto::tools::RollbackResponse>, tonic::Status> {
        let req = request.into_inner();
        info!("Rolling back execution: {}", req.execution_id);

        let mut state = self.state.lock().await;
        let result = state
            .backup_manager
            .rollback(&req.execution_id)
            .await
            .map_err(|e| tonic::Status::internal(format!("Rollback failed: {e}")))?;

        Ok(tonic::Response::new(proto::tools::RollbackResponse {
            success: result,
            error: String::new(),
        }))
    }

    async fn register(
        &self,
        request: tonic::Request<proto::tools::RegisterToolRequest>,
    ) -> Result<tonic::Response<proto::tools::RegisterToolResponse>, tonic::Status> {
        let req = request.into_inner();
        let tool = req
            .tool
            .ok_or_else(|| tonic::Status::invalid_argument("Missing tool definition"))?;

        info!("Registering external tool: {}", tool.name);

        let mut state = self.state.lock().await;
        state.registry.register_tool(tool);

        Ok(tonic::Response::new(
            proto::tools::RegisterToolResponse {
                accepted: true,
                error: String::new(),
            },
        ))
    }

    async fn deregister(
        &self,
        request: tonic::Request<proto::tools::DeregisterToolRequest>,
    ) -> Result<tonic::Response<proto::tools::Status>, tonic::Status> {
        let req = request.into_inner();
        let mut state = self.state.lock().await;
        state.registry.deregister_tool(&req.tool_name);

        Ok(tonic::Response::new(proto::tools::Status {
            success: true,
            message: format!("Tool {} deregistered", req.tool_name),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .compact()
        .init();

    info!("aiOS Tool Registry starting...");

    // Initialize state with all built-in tools registered
    let mut reg = registry::Registry::new();
    register_builtin_tools(&mut reg);

    let state = Arc::new(Mutex::new(ToolRegistryState {
        registry: reg,
        executor: executor::Executor::new(),
        audit_log: audit::AuditLog::new("/var/lib/aios/ledger/audit.db")?,
        backup_manager: backup::BackupManager::new("/var/lib/aios/cache/backups"),
    }));

    let service = ToolRegistryService { state };

    let addr: SocketAddr = "0.0.0.0:50052".parse()?;
    info!("Tool Registry gRPC server listening on {addr}");

    Server::builder()
        .add_service(ToolRegistryServer::new(service))
        .serve(addr)
        .await
        .context("Tool Registry gRPC server failed")?;

    Ok(())
}

/// Register all built-in system tools
fn register_builtin_tools(reg: &mut registry::Registry) {
    // Filesystem tools
    fs::register_tools(reg);
    // Process tools
    process::register_tools(reg);
    // Service tools
    service::register_tools(reg);
    // Network tools
    net::register_tools(reg);
    // Firewall tools
    firewall::register_tools(reg);
    // Package tools
    pkg::register_tools(reg);
    // Security tools
    sec::register_tools(reg);
    // Monitor tools
    monitor::register_tools(reg);
    // Hardware tools
    hw::register_tools(reg);

    info!("Registered {} built-in tools", reg.tool_count());
}
