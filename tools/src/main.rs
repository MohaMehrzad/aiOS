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
use tracing::{info, warn};

mod registry;
mod executor;
mod audit;
mod backup;
mod schema;
pub mod capabilities;
pub mod secrets;
pub mod sandbox;
pub mod firewall_apply;
pub mod fs;
pub mod process;
pub mod service;
pub mod net;
pub mod firewall;
pub mod pkg;
pub mod sec;
pub mod monitor;
pub mod hw;
pub mod web;
pub mod git;
pub mod code;
pub mod self_update;
pub mod plugin;
pub mod container;
pub mod email;

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
            ref mut registry,
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
                req.clone(),
            )
            .await
            .map_err(|e| tonic::Status::internal(format!("Execution failed: {e}")))?;

        // Plugin execution fallback: if no handler registered and tool is a plugin,
        // try running the plugin script directly
        if !response.success
            && response.error.contains("No handler registered")
            && req.tool_name.starts_with("plugin.")
        {
            let short_name = req.tool_name.strip_prefix("plugin.").unwrap_or(&req.tool_name);
            let script_path = format!("{}/{}.py", plugin::PLUGIN_DIR, short_name);

            if std::path::Path::new(&script_path).exists() {
                info!("Falling back to plugin script execution: {}", script_path);
                let sandbox = sandbox::Sandbox::new(sandbox::ResourceLimits {
                    allow_network: true,
                    max_cpu_time: std::time::Duration::from_secs(30),
                    writable_paths: vec!["/tmp".to_string()],
                    ..Default::default()
                });

                match sandbox
                    .execute("python3", &[&script_path], &req.input_json)
                    .await
                {
                    Ok(result) => {
                        audit_log.record(
                            &response.execution_id,
                            &req.tool_name,
                            &req.agent_id,
                            &req.task_id,
                            &format!("Plugin fallback: {}", req.reason),
                            result.success,
                            result.duration_ms as i64,
                        );
                        return Ok(tonic::Response::new(
                            proto::tools::ExecuteResponse {
                                success: result.success,
                                output_json: result.output,
                                error: result.error,
                                execution_id: response.execution_id,
                                duration_ms: result.duration_ms as i64,
                                backup_id: String::new(),
                            },
                        ));
                    }
                    Err(e) => {
                        warn!("Plugin script execution failed: {e}");
                    }
                }
            }
        }

        // After plugin.create succeeds, re-scan plugins to register the new tool
        if response.success && req.tool_name == "plugin.create" {
            info!("Plugin created successfully, rescanning plugin directory");
            plugin::scan_and_register_plugins(registry);
        }

        // Plugin chaining: if a plugin succeeded, check metadata for next_plugins
        if response.success && req.tool_name.starts_with("plugin.") {
            let short_name = req.tool_name.strip_prefix("plugin.").unwrap_or(&req.tool_name);
            let meta_path = format!("{}/{}.meta.json", plugin::PLUGIN_DIR, short_name);
            if let Ok(meta_contents) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<plugin::PluginMetadata>(&meta_contents) {
                    if !meta.next_plugins.is_empty() {
                        info!(
                            "Plugin chaining: {} -> {:?} (mode: {})",
                            req.tool_name, meta.next_plugins, meta.output_mode
                        );
                        let chain_input = if meta.output_mode == "merge" {
                            // Merge: combine original input with output
                            let mut merged = serde_json::from_slice::<serde_json::Value>(
                                &req.input_json,
                            )
                            .unwrap_or(serde_json::Value::Object(Default::default()));
                            if let Ok(output_val) =
                                serde_json::from_slice::<serde_json::Value>(&response.output_json)
                            {
                                if let (Some(m), Some(o)) =
                                    (merged.as_object_mut(), output_val.as_object())
                                {
                                    for (k, v) in o {
                                        m.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                            serde_json::to_vec(&merged).unwrap_or_default()
                        } else {
                            // Pipe: pass output as next plugin's input
                            response.output_json.clone()
                        };

                        for next_plugin in &meta.next_plugins {
                            let next_tool = if next_plugin.starts_with("plugin.") {
                                next_plugin.clone()
                            } else {
                                format!("plugin.{next_plugin}")
                            };
                            info!("Chaining to: {next_tool}");
                            let chain_req = proto::tools::ExecuteRequest {
                                tool_name: next_tool.clone(),
                                input_json: chain_input.clone(),
                                agent_id: req.agent_id.clone(),
                                task_id: req.task_id.clone(),
                                reason: format!("Chained from {}", req.tool_name),
                            };
                            let chain_resp = executor
                                .execute(registry, audit_log, backup_manager, chain_req)
                                .await;
                            match chain_resp {
                                Ok(r) if r.success => {
                                    info!("Chained plugin {next_tool} succeeded");
                                }
                                Ok(r) => {
                                    warn!("Chained plugin {next_tool} failed: {}", r.error);
                                }
                                Err(e) => {
                                    warn!("Chained plugin {next_tool} error: {e}");
                                }
                            }
                        }
                    }
                }
            }
        }

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

    // Load any previously-created plugins from disk
    plugin::scan_and_register_plugins(&mut reg);

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
    // Web connectivity tools
    web::register_tools(reg);
    // Git tools
    git::register_tools(reg);
    // Code generation tools
    code::register_tools(reg);
    // Self-update tools
    self_update::register_tools(reg);
    // Plugin meta-tools
    plugin::register_tools(reg);
    // Container tools (Podman)
    container::register_tools(reg);
    // Email tools
    email::register_tools(reg);

    info!("Registered {} built-in tools", reg.tool_count());
}
