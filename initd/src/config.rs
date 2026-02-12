//! aiOS configuration loading and parsing

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

const DEFAULT_CONFIG_PATH: &str = "/etc/aios/config.toml";

/// Root configuration structure
#[derive(Debug, Deserialize)]
pub struct AiosConfig {
    #[serde(default)]
    pub system: SystemConfig,
    #[serde(default)]
    pub boot: BootConfig,
    #[serde(default)]
    pub models: ModelsConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub networking: NetworkingConfig,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Deserialize)]
pub struct SystemConfig {
    #[serde(default = "default_hostname")]
    pub hostname: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_file")]
    pub log_file: String,
    #[serde(default = "default_autonomy_level")]
    pub autonomy_level: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            hostname: default_hostname(),
            log_level: default_log_level(),
            log_file: default_log_file(),
            autonomy_level: default_autonomy_level(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BootConfig {
    #[serde(default = "default_init_timeout")]
    pub init_timeout_seconds: u64,
    #[serde(default)]
    pub debug_shell: bool,
    #[serde(default = "default_clean_shutdown_flag")]
    pub clean_shutdown_flag: String,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            init_timeout_seconds: default_init_timeout(),
            debug_shell: false,
            clean_shutdown_flag: default_clean_shutdown_flag(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct ModelsConfig {
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default = "default_model_dir")]
    pub model_dir: String,
    #[serde(default = "default_llama_server_binary")]
    pub llama_server_binary: String,
    pub operational: Option<ModelConfig>,
    pub tactical: Option<ModelConfig>,
    pub tactical_alt: Option<ModelConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModelConfig {
    pub file: String,
    #[serde(default = "default_true")]
    pub always_loaded: bool,
    #[serde(default)]
    pub load_on_demand: bool,
    #[serde(default = "default_context_length")]
    pub context_length: u32,
    #[serde(default = "default_threads")]
    pub threads: u32,
    #[serde(default = "default_gpu_layers")]
    pub gpu_layers: i32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_idle_timeout")]
    pub unload_after_idle_minutes: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct ApiConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub claude: Option<ApiProviderConfig>,
    pub openai: Option<ApiProviderConfig>,
    pub cache: Option<CacheConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiProviderConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_api_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_api_temperature")]
    pub temperature: f32,
    #[serde(default = "default_monthly_budget")]
    pub monthly_budget_usd: f64,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_rpm: u32,
    #[serde(default = "default_api_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub fallback_only: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CacheConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cache_max")]
    pub max_entries: u32,
    #[serde(default = "default_cache_ttl")]
    pub default_ttl_hours: u32,
    #[serde(default = "default_doc_ttl")]
    pub documentation_ttl_hours: u32,
    #[serde(default = "default_true")]
    pub never_cache_security: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct MemoryConfig {
    #[serde(default = "default_op_max")]
    pub operational_max_entries: u32,
    #[serde(default = "default_working_db")]
    pub working_db: String,
    #[serde(default = "default_longterm_db")]
    pub longterm_db: String,
    #[serde(default = "default_vector_dir")]
    pub vector_db_dir: String,
    #[serde(default = "default_retention")]
    pub working_retention_days: u32,
    #[serde(default = "default_vacuum")]
    pub vacuum_interval_days: u32,
    #[serde(default = "default_context_tokens")]
    pub context_max_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_capability_mode")]
    pub capability_mode: String,
    #[serde(default = "default_true")]
    pub audit_all_tool_calls: bool,
    #[serde(default = "default_audit_db")]
    pub audit_db: String,
    #[serde(default = "default_true")]
    pub sandbox_agents: bool,
    #[serde(default = "default_true")]
    pub sandbox_untrusted_tasks: bool,
    #[serde(default = "default_true")]
    pub auto_patch: bool,
    #[serde(default = "default_secrets_file")]
    pub secrets_file: String,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            capability_mode: default_capability_mode(),
            audit_all_tool_calls: true,
            audit_db: default_audit_db(),
            sandbox_agents: true,
            sandbox_untrusted_tasks: true,
            auto_patch: true,
            secrets_file: default_secrets_file(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct NetworkingConfig {
    #[serde(default = "default_mgmt_port")]
    pub management_port: u16,
    #[serde(default = "default_true")]
    pub management_tls: bool,
    #[serde(default = "default_mgmt_subnet")]
    pub management_subnet: String,
    #[serde(default = "default_dhcp_timeout")]
    pub dhcp_timeout_seconds: u32,
    #[serde(default = "default_dns_servers")]
    pub dns_servers: Vec<String>,
    #[serde(default = "default_true")]
    pub dns_over_tls: bool,
    #[serde(default = "default_firewall_policy")]
    pub firewall_default_policy: String,
    #[serde(default = "default_true")]
    pub allow_outbound_https: bool,
}

impl Default for NetworkingConfig {
    fn default() -> Self {
        Self {
            management_port: default_mgmt_port(),
            management_tls: true,
            management_subnet: default_mgmt_subnet(),
            dhcp_timeout_seconds: default_dhcp_timeout(),
            dns_servers: default_dns_servers(),
            dns_over_tls: true,
            firewall_default_policy: default_firewall_policy(),
            allow_outbound_https: true,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AgentsConfig {
    #[serde(default = "default_agent_config_dir")]
    pub config_dir: String,
    #[serde(default = "default_prompts_dir")]
    pub prompts_dir: String,
    #[serde(default = "default_max_instances")]
    pub max_instances_per_type: u32,
    #[serde(default = "default_heartbeat_timeout")]
    pub heartbeat_timeout_seconds: u64,
    #[serde(default = "default_max_restarts")]
    pub max_restart_attempts: u32,
    #[serde(default = "default_restart_window")]
    pub restart_window_seconds: u64,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            config_dir: default_agent_config_dir(),
            prompts_dir: default_prompts_dir(),
            max_instances_per_type: default_max_instances(),
            heartbeat_timeout_seconds: default_heartbeat_timeout(),
            max_restart_attempts: default_max_restarts(),
            restart_window_seconds: default_restart_window(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct MonitoringConfig {
    #[serde(default = "default_health_interval")]
    pub health_check_interval_seconds: u64,
    #[serde(default = "default_metric_interval")]
    pub metric_collection_interval_seconds: u64,
    #[serde(default = "default_true")]
    pub anomaly_detection_enabled: bool,
    #[serde(default = "default_log_max_size")]
    pub log_rotation_max_size_mb: u64,
    #[serde(default = "default_log_keep")]
    pub log_rotation_keep_files: u32,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            health_check_interval_seconds: default_health_interval(),
            metric_collection_interval_seconds: default_metric_interval(),
            anomaly_detection_enabled: true,
            log_rotation_max_size_mb: default_log_max_size(),
            log_rotation_keep_files: default_log_keep(),
        }
    }
}

// Default value functions
fn default_hostname() -> String { "aios".into() }
fn default_log_level() -> String { "info".into() }
fn default_log_file() -> String { "/var/log/aios/system.log".into() }
fn default_autonomy_level() -> String { "full".into() }
fn default_init_timeout() -> u64 { 300 }
fn default_clean_shutdown_flag() -> String { "/var/lib/aios/clean_shutdown".into() }
fn default_runtime() -> String { "llama-cpp".into() }
fn default_model_dir() -> String { "/var/lib/aios/models".into() }
fn default_llama_server_binary() -> String { "/usr/bin/llama-server".into() }
fn default_true() -> bool { true }
fn default_context_length() -> u32 { 2048 }
fn default_threads() -> u32 { 2 }
fn default_gpu_layers() -> i32 { -1 }
fn default_max_tokens() -> u32 { 512 }
fn default_temperature() -> f32 { 0.1 }
fn default_idle_timeout() -> u64 { 5 }
fn default_api_max_tokens() -> u32 { 4096 }
fn default_api_temperature() -> f32 { 0.3 }
fn default_monthly_budget() -> f64 { 100.0 }
fn default_rate_limit() -> u32 { 50 }
fn default_api_timeout() -> u64 { 30 }
fn default_cache_max() -> u32 { 1000 }
fn default_cache_ttl() -> u32 { 1 }
fn default_doc_ttl() -> u32 { 24 }
fn default_op_max() -> u32 { 10000 }
fn default_working_db() -> String { "/var/lib/aios/memory/working.db".into() }
fn default_longterm_db() -> String { "/var/lib/aios/memory/longterm.db".into() }
fn default_vector_dir() -> String { "/var/lib/aios/vectors".into() }
fn default_retention() -> u32 { 30 }
fn default_vacuum() -> u32 { 7 }
fn default_context_tokens() -> u32 { 4000 }
fn default_capability_mode() -> String { "strict".into() }
fn default_audit_db() -> String { "/var/lib/aios/ledger/audit.db".into() }
fn default_secrets_file() -> String { "/etc/aios/secrets.enc".into() }
fn default_mgmt_port() -> u16 { 9090 }
fn default_mgmt_subnet() -> String { "0.0.0.0/0".into() }
fn default_dhcp_timeout() -> u32 { 30 }
fn default_dns_servers() -> Vec<String> { vec!["1.1.1.1".into(), "8.8.8.8".into()] }
fn default_firewall_policy() -> String { "deny".into() }
fn default_agent_config_dir() -> String { "/etc/aios/agents".into() }
fn default_prompts_dir() -> String { "/etc/aios/agents/prompts".into() }
fn default_max_instances() -> u32 { 3 }
fn default_heartbeat_timeout() -> u64 { 15 }
fn default_max_restarts() -> u32 { 5 }
fn default_restart_window() -> u64 { 300 }
fn default_health_interval() -> u64 { 30 }
fn default_metric_interval() -> u64 { 10 }
fn default_log_max_size() -> u64 { 100 }
fn default_log_keep() -> u32 { 10 }

/// Load configuration from /etc/aios/config.toml
pub fn load_config() -> Result<AiosConfig> {
    let config_path = std::env::var("AIOS_CONFIG")
        .unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());

    if Path::new(&config_path).exists() {
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {config_path}"))?;
        let config: AiosConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {config_path}"))?;
        Ok(config)
    } else {
        tracing::warn!("Config file not found at {config_path}, using defaults");
        Ok(AiosConfig {
            system: SystemConfig::default(),
            boot: BootConfig::default(),
            models: ModelsConfig::default(),
            api: ApiConfig::default(),
            memory: MemoryConfig::default(),
            security: SecurityConfig::default(),
            networking: NetworkingConfig::default(),
            agents: AgentsConfig::default(),
            monitoring: MonitoringConfig::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AiosConfig {
            system: SystemConfig::default(),
            boot: BootConfig::default(),
            models: ModelsConfig::default(),
            api: ApiConfig::default(),
            memory: MemoryConfig::default(),
            security: SecurityConfig::default(),
            networking: NetworkingConfig::default(),
            agents: AgentsConfig::default(),
            monitoring: MonitoringConfig::default(),
        };
        assert_eq!(config.system.hostname, "aios");
        assert_eq!(config.boot.init_timeout_seconds, 300);
        assert!(!config.boot.debug_shell);
    }

    #[test]
    fn test_parse_minimal_config() {
        let toml_str = r#"
[system]
hostname = "test-aios"
log_level = "debug"

[boot]
debug_shell = true
"#;
        let config: AiosConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.system.hostname, "test-aios");
        assert_eq!(config.system.log_level, "debug");
        assert!(config.boot.debug_shell);
    }

    #[test]
    fn test_parse_full_config() {
        let toml_str = r#"
[system]
hostname = "aios"
log_level = "info"
log_file = "/var/log/aios/system.log"
autonomy_level = "full"

[boot]
init_timeout_seconds = 300
debug_shell = false
clean_shutdown_flag = "/var/lib/aios/clean_shutdown"

[models]
runtime = "llama-cpp"
model_dir = "/var/lib/aios/models"
llama_server_binary = "/usr/bin/llama-server"

[models.operational]
file = "tinyllama-1.1b-chat.Q4_K_M.gguf"
always_loaded = true
context_length = 2048
threads = 2
gpu_layers = -1
max_tokens = 512
temperature = 0.1

[memory]
operational_max_entries = 10000
working_db = "/var/lib/aios/memory/working.db"

[security]
capability_mode = "strict"
audit_all_tool_calls = true
"#;
        let config: AiosConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.system.hostname, "aios");
        assert!(config.models.operational.is_some());
        let op = config.models.operational.as_ref().unwrap();
        assert_eq!(op.context_length, 2048);
        assert!(op.always_loaded);
    }
}
