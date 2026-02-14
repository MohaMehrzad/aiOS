//! Firewall Rule Application — applies firewall rules from configuration
//!
//! Parses firewall-rules.toml into iptables/nftables commands.
//! Supports dynamic rule changes with rollback capability.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::{info, warn};

/// A firewall rule from configuration
#[derive(Debug, Clone, Deserialize)]
pub struct FirewallRule {
    pub name: String,
    pub action: FirewallAction,
    pub direction: Direction,
    pub protocol: Option<String>,
    pub port: Option<u16>,
    pub port_range: Option<(u16, u16)>,
    pub source: Option<String>,
    pub destination: Option<String>,
    pub comment: Option<String>,
}

/// Firewall action
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FirewallAction {
    Accept,
    Drop,
    Reject,
    Log,
}

/// Traffic direction
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Input,
    Output,
    Forward,
}

/// Configuration file structure
#[derive(Debug, Deserialize)]
struct FirewallConfig {
    #[serde(default)]
    default_policy: String,
    #[serde(default)]
    rules: Vec<FirewallRule>,
}

/// Applied rule with rollback info
#[derive(Debug)]
struct AppliedRule {
    rule: FirewallRule,
    nftables_command: String,
    applied_at: i64,
}

/// Manages firewall rule application
pub struct FirewallApplicator {
    config_path: PathBuf,
    applied_rules: Vec<AppliedRule>,
    use_nftables: bool,
}

impl FirewallApplicator {
    pub fn new(config_path: &str) -> Self {
        Self {
            config_path: PathBuf::from(config_path),
            applied_rules: Vec::new(),
            use_nftables: Self::detect_nftables(),
        }
    }

    /// Detect if nftables is available
    fn detect_nftables() -> bool {
        std::process::Command::new("nft")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Load rules from configuration file
    pub fn load_config(&self) -> Result<Vec<FirewallRule>> {
        if !self.config_path.exists() {
            warn!("Firewall config not found: {}", self.config_path.display());
            return Ok(Vec::new());
        }

        let contents =
            std::fs::read_to_string(&self.config_path).context("Failed to read firewall config")?;

        let config: FirewallConfig =
            toml::from_str(&contents).context("Failed to parse firewall config")?;

        info!(
            "Loaded {} firewall rules (default policy: {})",
            config.rules.len(),
            config.default_policy
        );

        Ok(config.rules)
    }

    /// Convert a rule to an nftables command
    pub fn rule_to_nftables(&self, rule: &FirewallRule) -> String {
        let chain = match rule.direction {
            Direction::Input => "input",
            Direction::Output => "output",
            Direction::Forward => "forward",
        };

        let action = match rule.action {
            FirewallAction::Accept => "accept",
            FirewallAction::Drop => "drop",
            FirewallAction::Reject => "reject",
            FirewallAction::Log => "log",
        };

        let mut parts = vec![format!("nft add rule inet aios {chain}")];

        if let Some(ref proto) = rule.protocol {
            parts.push(format!("{proto} dport"));
            if let Some(port) = rule.port {
                parts.push(port.to_string());
            } else if let Some((start, end)) = rule.port_range {
                parts.push(format!("{start}-{end}"));
            }
        }

        if let Some(ref src) = rule.source {
            parts.push(format!("ip saddr {src}"));
        }

        if let Some(ref dst) = rule.destination {
            parts.push(format!("ip daddr {dst}"));
        }

        if let Some(ref comment) = rule.comment {
            parts.push(format!("comment \"{comment}\""));
        }

        parts.push(action.to_string());
        parts.join(" ")
    }

    /// Convert a rule to an iptables command
    pub fn rule_to_iptables(&self, rule: &FirewallRule) -> String {
        let chain = match rule.direction {
            Direction::Input => "INPUT",
            Direction::Output => "OUTPUT",
            Direction::Forward => "FORWARD",
        };

        let action = match rule.action {
            FirewallAction::Accept => "ACCEPT",
            FirewallAction::Drop => "DROP",
            FirewallAction::Reject => "REJECT",
            FirewallAction::Log => "LOG",
        };

        let mut parts = vec![format!("iptables -A {chain}")];

        if let Some(ref proto) = rule.protocol {
            parts.push(format!("-p {proto}"));
            if let Some(port) = rule.port {
                parts.push(format!("--dport {port}"));
            } else if let Some((start, end)) = rule.port_range {
                parts.push(format!("--dport {start}:{end}"));
            }
        }

        if let Some(ref src) = rule.source {
            parts.push(format!("-s {src}"));
        }

        if let Some(ref dst) = rule.destination {
            parts.push(format!("-d {dst}"));
        }

        parts.push(format!("-j {action}"));

        if let Some(ref comment) = rule.comment {
            parts.push(format!("-m comment --comment \"{comment}\""));
        }

        parts.join(" ")
    }

    /// Generate the command for a rule based on available tools
    pub fn generate_command(&self, rule: &FirewallRule) -> String {
        if self.use_nftables {
            self.rule_to_nftables(rule)
        } else {
            self.rule_to_iptables(rule)
        }
    }

    /// Apply all rules from config (dry run — returns commands)
    pub fn apply_all_dry_run(&self) -> Result<Vec<String>> {
        let rules = self.load_config()?;
        Ok(rules.iter().map(|r| self.generate_command(r)).collect())
    }

    /// Record an applied rule for rollback
    pub fn record_applied(&mut self, rule: FirewallRule) {
        let command = self.generate_command(&rule);
        self.applied_rules.push(AppliedRule {
            rule,
            nftables_command: command,
            applied_at: chrono::Utc::now().timestamp(),
        });
    }

    /// Generate rollback commands for all applied rules
    pub fn generate_rollback_commands(&self) -> Vec<String> {
        self.applied_rules
            .iter()
            .rev()
            .map(|applied| {
                // Replace "add" with "delete" for nftables, "-A" with "-D" for iptables
                if self.use_nftables {
                    applied.nftables_command.replace("add rule", "delete rule")
                } else {
                    applied.nftables_command.replace("-A ", "-D ")
                }
            })
            .collect()
    }

    /// Get count of applied rules
    pub fn applied_count(&self) -> usize {
        self.applied_rules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rule(
        name: &str,
        action: FirewallAction,
        direction: Direction,
        proto: Option<&str>,
        port: Option<u16>,
    ) -> FirewallRule {
        FirewallRule {
            name: name.to_string(),
            action,
            direction,
            protocol: proto.map(|s| s.to_string()),
            port,
            port_range: None,
            source: None,
            destination: None,
            comment: None,
        }
    }

    #[test]
    fn test_applicator_new() {
        let app = FirewallApplicator::new("/etc/aios/firewall-rules.toml");
        assert_eq!(app.applied_count(), 0);
    }

    #[test]
    fn test_rule_to_iptables() {
        let mut app = FirewallApplicator::new("/nonexistent");
        app.use_nftables = false;

        let rule = make_rule(
            "allow-ssh",
            FirewallAction::Accept,
            Direction::Input,
            Some("tcp"),
            Some(22),
        );
        let cmd = app.rule_to_iptables(&rule);
        assert!(cmd.contains("iptables -A INPUT"));
        assert!(cmd.contains("-p tcp"));
        assert!(cmd.contains("--dport 22"));
        assert!(cmd.contains("-j ACCEPT"));
    }

    #[test]
    fn test_rule_to_nftables() {
        let mut app = FirewallApplicator::new("/nonexistent");
        app.use_nftables = true;

        let rule = make_rule(
            "allow-http",
            FirewallAction::Accept,
            Direction::Input,
            Some("tcp"),
            Some(80),
        );
        let cmd = app.rule_to_nftables(&rule);
        assert!(cmd.contains("nft add rule"));
        assert!(cmd.contains("input"));
        assert!(cmd.contains("tcp dport"));
        assert!(cmd.contains("80"));
        assert!(cmd.contains("accept"));
    }

    #[test]
    fn test_rule_with_source() {
        let mut app = FirewallApplicator::new("/nonexistent");
        app.use_nftables = false;

        let rule = FirewallRule {
            name: "block-ip".to_string(),
            action: FirewallAction::Drop,
            direction: Direction::Input,
            protocol: None,
            port: None,
            port_range: None,
            source: Some("10.0.0.0/8".to_string()),
            destination: None,
            comment: Some("Block private range".to_string()),
        };

        let cmd = app.rule_to_iptables(&rule);
        assert!(cmd.contains("-s 10.0.0.0/8"));
        assert!(cmd.contains("-j DROP"));
    }

    #[test]
    fn test_record_and_rollback() {
        let mut app = FirewallApplicator::new("/nonexistent");
        app.use_nftables = false;

        let rule = make_rule(
            "allow-ssh",
            FirewallAction::Accept,
            Direction::Input,
            Some("tcp"),
            Some(22),
        );
        app.record_applied(rule);
        assert_eq!(app.applied_count(), 1);

        let rollback = app.generate_rollback_commands();
        assert_eq!(rollback.len(), 1);
        assert!(rollback[0].contains("-D "));
    }

    #[test]
    fn test_load_config_missing() {
        let app = FirewallApplicator::new("/nonexistent/firewall.toml");
        let rules = app.load_config().unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn test_port_range_nftables() {
        let app = FirewallApplicator::new("/nonexistent");
        let rule = FirewallRule {
            name: "ephemeral".to_string(),
            action: FirewallAction::Accept,
            direction: Direction::Input,
            protocol: Some("tcp".to_string()),
            port: None,
            port_range: Some((1024, 65535)),
            source: None,
            destination: None,
            comment: None,
        };

        let cmd = app.rule_to_nftables(&rule);
        assert!(cmd.contains("1024-65535"));
    }
}
