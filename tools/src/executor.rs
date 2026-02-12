//! Tool execution pipeline
//!
//! Pipeline: validate input → check permissions → backup → execute → audit

use anyhow::Result;
use std::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::audit::AuditLog;
use crate::backup::BackupManager;
use crate::proto::tools::{ExecuteRequest, ExecuteResponse};
use crate::registry::Registry;

/// Executes tools through the full pipeline
pub struct Executor {
    /// Map of tool name → handler function
    handlers: std::collections::HashMap<String, ToolHandler>,
}

/// A tool handler function
type ToolHandler = Box<dyn Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync>;

impl Executor {
    pub fn new() -> Self {
        let mut executor = Self {
            handlers: std::collections::HashMap::new(),
        };
        executor.register_handlers();
        executor
    }

    /// Register all built-in tool handlers
    fn register_handlers(&mut self) {
        // Filesystem tools
        self.handlers.insert(
            "fs.read".into(),
            Box::new(|input| crate::fs::read::execute(input)),
        );
        self.handlers.insert(
            "fs.write".into(),
            Box::new(|input| crate::fs::write::execute(input)),
        );
        self.handlers.insert(
            "fs.delete".into(),
            Box::new(|input| crate::fs::delete::execute(input)),
        );
        self.handlers.insert(
            "fs.list".into(),
            Box::new(|input| crate::fs::list::execute(input)),
        );
        self.handlers.insert(
            "fs.stat".into(),
            Box::new(|input| crate::fs::stat::execute(input)),
        );
        self.handlers.insert(
            "fs.mkdir".into(),
            Box::new(|input| crate::fs::mkdir::execute(input)),
        );
        self.handlers.insert(
            "fs.move".into(),
            Box::new(|input| crate::fs::move_file::execute(input)),
        );
        self.handlers.insert(
            "fs.copy".into(),
            Box::new(|input| crate::fs::copy::execute(input)),
        );
        self.handlers.insert(
            "fs.chmod".into(),
            Box::new(|input| crate::fs::chmod::execute(input)),
        );
        self.handlers.insert(
            "fs.chown".into(),
            Box::new(|input| crate::fs::chown::execute(input)),
        );
        self.handlers.insert(
            "fs.symlink".into(),
            Box::new(|input| crate::fs::symlink::execute(input)),
        );
        self.handlers.insert(
            "fs.search".into(),
            Box::new(|input| crate::fs::search::execute(input)),
        );
        self.handlers.insert(
            "fs.disk_usage".into(),
            Box::new(|input| crate::fs::disk_usage::execute(input)),
        );

        // Process tools
        self.handlers.insert(
            "process.list".into(),
            Box::new(|input| crate::process::list::execute(input)),
        );
        self.handlers.insert(
            "process.spawn".into(),
            Box::new(|input| crate::process::spawn::execute(input)),
        );
        self.handlers.insert(
            "process.kill".into(),
            Box::new(|input| crate::process::kill::execute(input)),
        );
        self.handlers.insert(
            "process.info".into(),
            Box::new(|input| crate::process::info::execute(input)),
        );
        self.handlers.insert(
            "process.signal".into(),
            Box::new(|input| crate::process::signal::execute(input)),
        );

        // Service tools
        self.handlers.insert(
            "service.list".into(),
            Box::new(|input| crate::service::list::execute(input)),
        );
        self.handlers.insert(
            "service.start".into(),
            Box::new(|input| crate::service::start::execute(input)),
        );
        self.handlers.insert(
            "service.stop".into(),
            Box::new(|input| crate::service::stop::execute(input)),
        );
        self.handlers.insert(
            "service.restart".into(),
            Box::new(|input| crate::service::restart::execute(input)),
        );
        self.handlers.insert(
            "service.status".into(),
            Box::new(|input| crate::service::status::execute(input)),
        );

        // Network tools
        self.handlers.insert(
            "net.interfaces".into(),
            Box::new(|input| crate::net::interfaces::execute(input)),
        );
        self.handlers.insert(
            "net.ping".into(),
            Box::new(|input| crate::net::ping::execute(input)),
        );
        self.handlers.insert(
            "net.dns".into(),
            Box::new(|input| crate::net::dns::execute(input)),
        );
        self.handlers.insert(
            "net.http_get".into(),
            Box::new(|input| crate::net::http_get::execute(input)),
        );
        self.handlers.insert(
            "net.port_scan".into(),
            Box::new(|input| crate::net::port_scan::execute(input)),
        );

        // Firewall tools
        self.handlers.insert(
            "firewall.rules".into(),
            Box::new(|input| crate::firewall::rules::execute(input)),
        );
        self.handlers.insert(
            "firewall.add_rule".into(),
            Box::new(|input| crate::firewall::add_rule::execute(input)),
        );
        self.handlers.insert(
            "firewall.delete_rule".into(),
            Box::new(|input| crate::firewall::delete_rule::execute(input)),
        );

        // Package tools
        self.handlers.insert(
            "pkg.install".into(),
            Box::new(|input| crate::pkg::install::execute(input)),
        );
        self.handlers.insert(
            "pkg.remove".into(),
            Box::new(|input| crate::pkg::remove::execute(input)),
        );
        self.handlers.insert(
            "pkg.search".into(),
            Box::new(|input| crate::pkg::search::execute(input)),
        );
        self.handlers.insert(
            "pkg.update".into(),
            Box::new(|input| crate::pkg::update::execute(input)),
        );
        self.handlers.insert(
            "pkg.list_installed".into(),
            Box::new(|input| crate::pkg::list_installed::execute(input)),
        );

        // Security tools
        self.handlers.insert(
            "sec.check_perms".into(),
            Box::new(|input| crate::sec::check_perms::execute(input)),
        );
        self.handlers.insert(
            "sec.audit_query".into(),
            Box::new(|input| crate::sec::audit_query::execute(input)),
        );

        // Monitor tools
        self.handlers.insert(
            "monitor.cpu".into(),
            Box::new(|input| crate::monitor::cpu::execute(input)),
        );
        self.handlers.insert(
            "monitor.memory".into(),
            Box::new(|input| crate::monitor::memory::execute(input)),
        );
        self.handlers.insert(
            "monitor.disk".into(),
            Box::new(|input| crate::monitor::disk::execute(input)),
        );
        self.handlers.insert(
            "monitor.network".into(),
            Box::new(|input| crate::monitor::network::execute(input)),
        );
        self.handlers.insert(
            "monitor.logs".into(),
            Box::new(|input| crate::monitor::logs::execute(input)),
        );

        // Hardware tools
        self.handlers.insert(
            "hw.info".into(),
            Box::new(|input| crate::hw::info::execute(input)),
        );
    }

    /// Execute a tool through the full pipeline
    pub async fn execute(
        &self,
        registry: &Registry,
        audit_log: &mut AuditLog,
        backup_manager: &mut BackupManager,
        request: ExecuteRequest,
    ) -> Result<ExecuteResponse> {
        let execution_id = Uuid::new_v4().to_string();
        let start = Instant::now();

        // 1. Validate: check tool exists
        let tool_def = registry
            .get_tool(&request.tool_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", request.tool_name))?;

        // 2. Check permissions (capability-based)
        // The orchestrator validates agent capabilities before routing; the tools
        // service logs the required capabilities for audit trail purposes.
        info!(
            "Permission check: agent={} tool={} required_caps={:?}",
            request.agent_id, request.tool_name, tool_def.required_capabilities
        );

        // 3. Pre-execution backup if tool is reversible
        let backup_id = if tool_def.reversible {
            let bid = backup_manager.create_backup(&execution_id, &request.tool_name, &request.input_json);
            Some(bid)
        } else {
            None
        };

        // 4. Execute the tool
        let result = if let Some(handler) = self.handlers.get(&request.tool_name) {
            match handler(&request.input_json) {
                Ok(output) => ExecuteResponse {
                    success: true,
                    output_json: output,
                    error: String::new(),
                    execution_id: execution_id.clone(),
                    duration_ms: start.elapsed().as_millis() as i64,
                    backup_id: backup_id.unwrap_or_default(),
                },
                Err(e) => ExecuteResponse {
                    success: false,
                    output_json: vec![],
                    error: e.to_string(),
                    execution_id: execution_id.clone(),
                    duration_ms: start.elapsed().as_millis() as i64,
                    backup_id: backup_id.unwrap_or_default(),
                },
            }
        } else {
            ExecuteResponse {
                success: false,
                output_json: vec![],
                error: format!("No handler registered for tool: {}", request.tool_name),
                execution_id: execution_id.clone(),
                duration_ms: start.elapsed().as_millis() as i64,
                backup_id: String::new(),
            }
        };

        // 5. Audit log
        audit_log.record(
            &execution_id,
            &request.tool_name,
            &request.agent_id,
            &request.task_id,
            &request.reason,
            result.success,
            result.duration_ms,
        );

        Ok(result)
    }
}
