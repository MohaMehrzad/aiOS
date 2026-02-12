//! Tool execution pipeline
//!
//! Pipeline: validate input → check capabilities → rate limit → backup → execute (sandbox) → audit

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

use crate::audit::AuditLog;
use crate::backup::BackupManager;
use crate::capabilities::CapabilityChecker;
use crate::proto::tools::{ExecuteRequest, ExecuteResponse};
use crate::registry::Registry;

/// Token bucket for rate limiting
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Rate limiter with per-agent and per-tool buckets
struct RateLimiter {
    agent_buckets: HashMap<String, TokenBucket>,
    tool_buckets: HashMap<String, TokenBucket>,
    agent_max_rps: f64,
    tool_max_rps: f64,
}

impl RateLimiter {
    fn new(agent_max_rps: f64, tool_max_rps: f64) -> Self {
        Self {
            agent_buckets: HashMap::new(),
            tool_buckets: HashMap::new(),
            agent_max_rps,
            tool_max_rps,
        }
    }

    fn check(&mut self, agent_id: &str, tool_name: &str) -> bool {
        let agent_ok = self
            .agent_buckets
            .entry(agent_id.to_string())
            .or_insert_with(|| TokenBucket::new(self.agent_max_rps * 2.0, self.agent_max_rps))
            .try_consume();

        let tool_ok = self
            .tool_buckets
            .entry(tool_name.to_string())
            .or_insert_with(|| TokenBucket::new(self.tool_max_rps * 2.0, self.tool_max_rps))
            .try_consume();

        agent_ok && tool_ok
    }
}

/// Executes tools through the full pipeline
pub struct Executor {
    /// Map of tool name → handler function
    handlers: HashMap<String, ToolHandler>,
    /// Capability checker for access control
    capability_checker: CapabilityChecker,
    /// Rate limiter
    rate_limiter: Mutex<RateLimiter>,
}

/// A tool handler function
type ToolHandler = Box<dyn Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync>;

impl Executor {
    pub fn new() -> Self {
        let mut executor = Self {
            handlers: HashMap::new(),
            capability_checker: CapabilityChecker::new(),
            rate_limiter: Mutex::new(RateLimiter::new(10.0, 50.0)),
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

        // Web connectivity tools
        self.handlers.insert(
            "web.http_request".into(),
            Box::new(|input| crate::web::http_request::execute(input)),
        );
        self.handlers.insert(
            "web.scrape".into(),
            Box::new(|input| crate::web::scrape::execute(input)),
        );
        self.handlers.insert(
            "web.webhook".into(),
            Box::new(|input| crate::web::webhook::execute(input)),
        );
        self.handlers.insert(
            "web.download".into(),
            Box::new(|input| crate::web::download::execute(input)),
        );
        self.handlers.insert(
            "web.api_call".into(),
            Box::new(|input| crate::web::api_call::execute(input)),
        );

        // Git tools
        self.handlers.insert(
            "git.init".into(),
            Box::new(|input| crate::git::operations::execute_init(input)),
        );
        self.handlers.insert(
            "git.clone".into(),
            Box::new(|input| crate::git::operations::execute_clone(input)),
        );
        self.handlers.insert(
            "git.add".into(),
            Box::new(|input| crate::git::operations::execute_add(input)),
        );
        self.handlers.insert(
            "git.commit".into(),
            Box::new(|input| crate::git::operations::execute_commit(input)),
        );
        self.handlers.insert(
            "git.push".into(),
            Box::new(|input| crate::git::operations::execute_push(input)),
        );
        self.handlers.insert(
            "git.pull".into(),
            Box::new(|input| crate::git::operations::execute_pull(input)),
        );
        self.handlers.insert(
            "git.branch".into(),
            Box::new(|input| crate::git::operations::execute_branch(input)),
        );
        self.handlers.insert(
            "git.status".into(),
            Box::new(|input| crate::git::operations::execute_status(input)),
        );
        self.handlers.insert(
            "git.log".into(),
            Box::new(|input| crate::git::operations::execute_log(input)),
        );
        self.handlers.insert(
            "git.diff".into(),
            Box::new(|input| crate::git::operations::execute_diff(input)),
        );

        // Code tools
        self.handlers.insert(
            "code.scaffold".into(),
            Box::new(|input| crate::code::scaffold::execute(input)),
        );
        self.handlers.insert(
            "code.generate".into(),
            Box::new(|input| crate::code::generate::execute(input)),
        );

        // Self-update tools
        self.handlers.insert(
            "self.inspect".into(),
            Box::new(|input| crate::self_update::inspect::execute(input)),
        );
        self.handlers.insert(
            "self.update".into(),
            Box::new(|input| crate::self_update::update::execute(input)),
        );
        self.handlers.insert(
            "self.rebuild".into(),
            Box::new(|input| crate::self_update::update::execute_rebuild(input)),
        );
        self.handlers.insert(
            "self.health".into(),
            Box::new(|input| crate::self_update::inspect::execute_health(input)),
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

        // 2. Capability-based access control
        let cap_result = self
            .capability_checker
            .check_permission(&request.agent_id, &request.tool_name);

        if !cap_result.allowed {
            warn!(
                "Capability denied: agent={} tool={} missing={:?}",
                request.agent_id, request.tool_name, cap_result.missing_capabilities
            );
            audit_log.record(
                &execution_id,
                &request.tool_name,
                &request.agent_id,
                &request.task_id,
                &request.reason,
                false,
                start.elapsed().as_millis() as i64,
            );
            return Ok(ExecuteResponse {
                success: false,
                output_json: vec![],
                error: format!(
                    "Capability denied: missing {:?}",
                    cap_result.missing_capabilities
                ),
                execution_id,
                duration_ms: start.elapsed().as_millis() as i64,
                backup_id: String::new(),
            });
        }

        // 3. Rate limiting
        {
            let mut limiter = self
                .rate_limiter
                .lock()
                .map_err(|e| anyhow::anyhow!("Rate limiter lock error: {e}"))?;
            if !limiter.check(&request.agent_id, &request.tool_name) {
                warn!(
                    "Rate limited: agent={} tool={}",
                    request.agent_id, request.tool_name
                );
                return Ok(ExecuteResponse {
                    success: false,
                    output_json: vec![],
                    error: "Rate limit exceeded".to_string(),
                    execution_id,
                    duration_ms: start.elapsed().as_millis() as i64,
                    backup_id: String::new(),
                });
            }
        }

        info!(
            "Executing: agent={} tool={} risk={:?}",
            request.agent_id, request.tool_name, cap_result.risk_level
        );

        // 4. Pre-execution backup if tool is reversible
        let backup_id = if tool_def.reversible {
            let bid = backup_manager.create_backup(&execution_id, &request.tool_name, &request.input_json);
            Some(bid)
        } else {
            None
        };

        // 5. Execute the tool (sandbox high-risk tools)
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

        // 6. Audit log
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
