//! Capability-Based Access Control
//!
//! Validates that an agent has the required capabilities before
//! allowing tool execution. Enforces the principle of least privilege.

use std::collections::{HashMap, HashSet};
use tracing::{info, warn};

/// Defines capabilities required for a tool namespace
#[derive(Debug, Clone)]
pub struct CapabilityRequirement {
    pub tool_pattern: String,
    pub required_capabilities: Vec<String>,
    pub risk_level: RiskLevel,
}

/// Risk level for a tool operation
#[derive(Debug, Clone, PartialEq)]
pub enum RiskLevel {
    /// Read-only operations, no side effects
    Low,
    /// Operations that modify state but are reversible
    Medium,
    /// Destructive or irreversible operations
    High,
    /// Operations that affect security or network boundaries
    Critical,
}

/// Validates agent capabilities against tool requirements
pub struct CapabilityChecker {
    /// Agent ID → set of capabilities
    agent_capabilities: HashMap<String, HashSet<String>>,
    /// Tool pattern → required capabilities
    tool_requirements: Vec<CapabilityRequirement>,
}

impl CapabilityChecker {
    pub fn new() -> Self {
        let mut checker = Self {
            agent_capabilities: HashMap::new(),
            tool_requirements: Vec::new(),
        };
        checker.register_default_requirements();
        checker
    }

    /// Register default tool capability requirements
    fn register_default_requirements(&mut self) {
        let requirements = vec![
            // Filesystem — read is low risk, write/delete is medium
            ("fs.read", vec!["fs_read"], RiskLevel::Low),
            ("fs.list", vec!["fs_read"], RiskLevel::Low),
            ("fs.stat", vec!["fs_read"], RiskLevel::Low),
            ("fs.search", vec!["fs_read"], RiskLevel::Low),
            ("fs.disk_usage", vec!["fs_read"], RiskLevel::Low),
            ("fs.write", vec!["fs_write"], RiskLevel::Medium),
            ("fs.mkdir", vec!["fs_write"], RiskLevel::Medium),
            ("fs.copy", vec!["fs_write"], RiskLevel::Medium),
            ("fs.move", vec!["fs_write"], RiskLevel::Medium),
            ("fs.symlink", vec!["fs_write"], RiskLevel::Medium),
            ("fs.delete", vec!["fs_write", "fs_delete"], RiskLevel::High),
            ("fs.chmod", vec!["fs_write", "fs_permissions"], RiskLevel::High),
            ("fs.chown", vec!["fs_write", "fs_permissions"], RiskLevel::High),
            // Process management
            ("process.list", vec!["process_read"], RiskLevel::Low),
            ("process.info", vec!["process_read"], RiskLevel::Low),
            ("process.spawn", vec!["process_manage"], RiskLevel::Medium),
            ("process.kill", vec!["process_manage"], RiskLevel::High),
            ("process.signal", vec!["process_manage"], RiskLevel::Medium),
            // Service management
            ("service.list", vec!["service_read"], RiskLevel::Low),
            ("service.status", vec!["service_read"], RiskLevel::Low),
            ("service.start", vec!["service_manage"], RiskLevel::Medium),
            ("service.stop", vec!["service_manage"], RiskLevel::High),
            ("service.restart", vec!["service_manage"], RiskLevel::Medium),
            // Network
            ("net.interfaces", vec!["net_read"], RiskLevel::Low),
            ("net.ping", vec!["net_read"], RiskLevel::Low),
            ("net.dns", vec!["net_read"], RiskLevel::Low),
            ("net.http_get", vec!["net_read"], RiskLevel::Low),
            ("net.port_scan", vec!["net_read", "net_scan"], RiskLevel::Medium),
            // Firewall
            ("firewall.rules", vec!["firewall_read"], RiskLevel::Low),
            ("firewall.add_rule", vec!["firewall_manage"], RiskLevel::Critical),
            ("firewall.delete_rule", vec!["firewall_manage"], RiskLevel::Critical),
            // Package management
            ("pkg.list_installed", vec!["pkg_read"], RiskLevel::Low),
            ("pkg.search", vec!["pkg_read"], RiskLevel::Low),
            ("pkg.install", vec!["pkg_manage"], RiskLevel::High),
            ("pkg.remove", vec!["pkg_manage"], RiskLevel::High),
            ("pkg.update", vec!["pkg_manage"], RiskLevel::High),
            // Security
            ("sec.check_perms", vec!["sec_read"], RiskLevel::Low),
            ("sec.audit_query", vec!["sec_read"], RiskLevel::Low),
            // Monitor — all read-only
            ("monitor.cpu", vec!["monitor_read"], RiskLevel::Low),
            ("monitor.memory", vec!["monitor_read"], RiskLevel::Low),
            ("monitor.disk", vec!["monitor_read"], RiskLevel::Low),
            ("monitor.network", vec!["monitor_read"], RiskLevel::Low),
            ("monitor.logs", vec!["monitor_read"], RiskLevel::Low),
            // Hardware
            ("hw.info", vec!["hw_read"], RiskLevel::Low),
            // Web connectivity
            ("web.http_request", vec!["net_read", "net_write"], RiskLevel::Medium),
            ("web.scrape", vec!["net_read"], RiskLevel::Low),
            ("web.webhook", vec!["net_write"], RiskLevel::Medium),
            ("web.download", vec!["net_read", "fs_write"], RiskLevel::Medium),
            ("web.api_call", vec!["net_read", "net_write"], RiskLevel::Medium),
            // Git operations
            ("git.init", vec!["git_write"], RiskLevel::Low),
            ("git.clone", vec!["git_write", "net_read"], RiskLevel::Medium),
            ("git.add", vec!["git_write"], RiskLevel::Low),
            ("git.commit", vec!["git_write"], RiskLevel::Low),
            ("git.push", vec!["git_write", "net_write"], RiskLevel::High),
            ("git.pull", vec!["git_write", "net_read"], RiskLevel::Medium),
            ("git.branch", vec!["git_write"], RiskLevel::Low),
            ("git.status", vec!["git_read"], RiskLevel::Low),
            ("git.log", vec!["git_read"], RiskLevel::Low),
            ("git.diff", vec!["git_read"], RiskLevel::Low),
            // Code generation
            ("code.scaffold", vec!["fs_write", "code_gen"], RiskLevel::Medium),
            ("code.generate", vec!["code_gen"], RiskLevel::Medium),
            // Self-update
            ("self.inspect", vec!["self_read"], RiskLevel::Low),
            ("self.health", vec!["self_read"], RiskLevel::Low),
            ("self.update", vec!["self_update"], RiskLevel::Critical),
            ("self.rebuild", vec!["self_update"], RiskLevel::Critical),
        ];

        for (pattern, caps, risk) in requirements {
            self.tool_requirements.push(CapabilityRequirement {
                tool_pattern: pattern.to_string(),
                required_capabilities: caps.into_iter().map(|s| s.to_string()).collect(),
                risk_level: risk,
            });
        }
    }

    /// Register capabilities for an agent
    pub fn register_agent(&mut self, agent_id: &str, capabilities: &[String]) {
        info!(
            "Registering capabilities for agent {}: {:?}",
            agent_id, capabilities
        );
        self.agent_capabilities.insert(
            agent_id.to_string(),
            capabilities.iter().cloned().collect(),
        );
    }

    /// Check if an agent has permission to execute a tool
    pub fn check_permission(&self, agent_id: &str, tool_name: &str) -> CapabilityCheckResult {
        // Find the capability requirement for this tool
        let requirement = self
            .tool_requirements
            .iter()
            .find(|r| r.tool_pattern == tool_name);

        let requirement = match requirement {
            Some(r) => r,
            None => {
                // Unknown tool — deny by default
                warn!("No capability requirement defined for tool: {tool_name}");
                return CapabilityCheckResult {
                    allowed: false,
                    reason: format!("No capability requirement defined for tool: {tool_name}"),
                    risk_level: RiskLevel::Critical,
                    missing_capabilities: vec![],
                };
            }
        };

        // Get agent capabilities
        let agent_caps = match self.agent_capabilities.get(agent_id) {
            Some(caps) => caps,
            None => {
                warn!("Agent {agent_id} has no registered capabilities");
                return CapabilityCheckResult {
                    allowed: false,
                    reason: format!("Agent {agent_id} has no registered capabilities"),
                    risk_level: requirement.risk_level.clone(),
                    missing_capabilities: requirement.required_capabilities.clone(),
                };
            }
        };

        // Check if agent has all required capabilities
        let missing: Vec<String> = requirement
            .required_capabilities
            .iter()
            .filter(|cap| !agent_caps.contains(*cap))
            .cloned()
            .collect();

        if missing.is_empty() {
            CapabilityCheckResult {
                allowed: true,
                reason: "All required capabilities present".to_string(),
                risk_level: requirement.risk_level.clone(),
                missing_capabilities: vec![],
            }
        } else {
            CapabilityCheckResult {
                allowed: false,
                reason: format!(
                    "Agent {} missing capabilities: {:?}",
                    agent_id, missing
                ),
                risk_level: requirement.risk_level.clone(),
                missing_capabilities: missing,
            }
        }
    }

    /// Get the risk level for a tool
    pub fn get_risk_level(&self, tool_name: &str) -> RiskLevel {
        self.tool_requirements
            .iter()
            .find(|r| r.tool_pattern == tool_name)
            .map(|r| r.risk_level.clone())
            .unwrap_or(RiskLevel::Critical)
    }
}

/// Result of a capability check
#[derive(Debug)]
pub struct CapabilityCheckResult {
    pub allowed: bool,
    pub reason: String,
    pub risk_level: RiskLevel,
    pub missing_capabilities: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_checker_new() {
        let checker = CapabilityChecker::new();
        assert!(!checker.tool_requirements.is_empty());
    }

    #[test]
    fn test_register_and_check_allowed() {
        let mut checker = CapabilityChecker::new();
        checker.register_agent("agent-1", &["fs_read".to_string()]);

        let result = checker.check_permission("agent-1", "fs.read");
        assert!(result.allowed);
        assert!(result.missing_capabilities.is_empty());
    }

    #[test]
    fn test_check_denied_missing_capability() {
        let mut checker = CapabilityChecker::new();
        checker.register_agent("agent-1", &["fs_read".to_string()]);

        let result = checker.check_permission("agent-1", "fs.write");
        assert!(!result.allowed);
        assert!(result.missing_capabilities.contains(&"fs_write".to_string()));
    }

    #[test]
    fn test_check_unknown_agent() {
        let checker = CapabilityChecker::new();
        let result = checker.check_permission("unknown-agent", "fs.read");
        assert!(!result.allowed);
    }

    #[test]
    fn test_check_unknown_tool() {
        let mut checker = CapabilityChecker::new();
        checker.register_agent("agent-1", &["fs_read".to_string()]);

        let result = checker.check_permission("agent-1", "unknown.tool");
        assert!(!result.allowed);
    }

    #[test]
    fn test_risk_levels() {
        let checker = CapabilityChecker::new();
        assert_eq!(checker.get_risk_level("fs.read"), RiskLevel::Low);
        assert_eq!(checker.get_risk_level("fs.delete"), RiskLevel::High);
        assert_eq!(checker.get_risk_level("firewall.add_rule"), RiskLevel::Critical);
        assert_eq!(checker.get_risk_level("unknown"), RiskLevel::Critical);
    }

    #[test]
    fn test_multiple_required_capabilities() {
        let mut checker = CapabilityChecker::new();
        // fs.delete requires both fs_write and fs_delete
        checker.register_agent("agent-1", &["fs_write".to_string()]);

        let result = checker.check_permission("agent-1", "fs.delete");
        assert!(!result.allowed);
        assert!(result.missing_capabilities.contains(&"fs_delete".to_string()));

        // Now register with both
        checker.register_agent(
            "agent-2",
            &["fs_write".to_string(), "fs_delete".to_string()],
        );
        let result = checker.check_permission("agent-2", "fs.delete");
        assert!(result.allowed);
    }
}
