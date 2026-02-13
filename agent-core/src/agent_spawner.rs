//! Agent Process Spawner — manages Python agent child processes
//!
//! Spawns Python agent processes based on configuration files,
//! monitors their health via gRPC heartbeats, and restarts failed agents.

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Configuration for a spawnable agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub agent_type: String,
    pub module: String,
    pub capabilities: Vec<String>,
    pub tool_namespaces: Vec<String>,
    pub max_restarts: u32,
    pub restart_delay: Duration,
}

/// State of a spawned agent process
#[derive(Debug)]
struct SpawnedAgent {
    config: AgentConfig,
    process: Option<tokio::process::Child>,
    restart_count: u32,
    last_heartbeat: Instant,
    started_at: Instant,
    status: AgentProcessStatus,
}

/// Status of an agent process
#[derive(Debug, Clone, PartialEq)]
pub enum AgentProcessStatus {
    Starting,
    Running,
    Failed,
    Stopped,
}

/// Manages spawning and monitoring of Python agent processes
pub struct AgentSpawner {
    agents: HashMap<String, SpawnedAgent>,
    config_dir: PathBuf,
    python_path: String,
    heartbeat_timeout: Duration,
}

impl AgentSpawner {
    pub fn new(config_dir: &str) -> Self {
        Self {
            agents: HashMap::new(),
            config_dir: PathBuf::from(config_dir),
            python_path: std::env::var("AIOS_PYTHON").unwrap_or_else(|_| "python3".to_string()),
            heartbeat_timeout: Duration::from_secs(30),
        }
    }

    /// Load agent configurations from the config directory
    pub fn load_configs(&mut self) -> Result<Vec<AgentConfig>> {
        let mut configs = Vec::new();

        if self.config_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&self.config_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |e| e == "toml") {
                        match std::fs::read_to_string(&path) {
                            Ok(contents) => {
                                match contents.parse::<toml::Table>() {
                                    Ok(table) => {
                                        let agent = table.get("agent");
                                        let agent_type = agent
                                            .and_then(|a| a.get("type"))
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("task");
                                        let name = agent
                                            .and_then(|a| a.get("name"))
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown");
                                        // Module name matches the Python file name (same as agent type)
                                        let module = format!("aios_agent.agents.{}", agent_type);
                                        let caps = table
                                            .get("capabilities")
                                            .and_then(|c| c.get("tools"))
                                            .and_then(|t| t.as_array())
                                            .map(|arr| {
                                                arr.iter()
                                                    .filter_map(|v| v.as_str())
                                                    .map(|s| {
                                                        s.split('.').next().unwrap_or(s).to_string()
                                                    })
                                                    .collect::<std::collections::HashSet<_>>()
                                                    .into_iter()
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default();

                                        let agent_name = format!("{}-agent", agent_type);
                                        info!(
                                            "Loaded agent config: {} (type: {}, module: {})",
                                            agent_name, agent_type, module
                                        );

                                        configs.push(AgentConfig {
                                            name: agent_name,
                                            agent_type: agent_type.to_string(),
                                            module,
                                            capabilities: vec![format!("{}_management", agent_type)],
                                            tool_namespaces: caps,
                                            max_restarts: 5,
                                            restart_delay: Duration::from_secs(5),
                                        });
                                    }
                                    Err(e) => {
                                        warn!("Failed to parse agent config {}: {e}", path.display());
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to read agent config {}: {e}", path.display());
                            }
                        }
                    }
                }
            }
        }

        // Fall back to defaults if no configs found
        if configs.is_empty() {
            configs.extend(vec![
                AgentConfig {
                    name: "system-agent".to_string(),
                    agent_type: "system".to_string(),
                    module: "aios_agent.agents.system".to_string(),
                    capabilities: vec!["system_management".to_string()],
                    tool_namespaces: vec!["fs".to_string(), "process".to_string(), "service".to_string()],
                    max_restarts: 5,
                    restart_delay: Duration::from_secs(5),
                },
                AgentConfig {
                    name: "network-agent".to_string(),
                    agent_type: "network".to_string(),
                    module: "aios_agent.agents.network".to_string(),
                    capabilities: vec!["network_management".to_string()],
                    tool_namespaces: vec!["net".to_string(), "firewall".to_string()],
                    max_restarts: 5,
                    restart_delay: Duration::from_secs(5),
                },
                AgentConfig {
                    name: "security-agent".to_string(),
                    agent_type: "security".to_string(),
                    module: "aios_agent.agents.security".to_string(),
                    capabilities: vec!["security_management".to_string()],
                    tool_namespaces: vec!["sec".to_string()],
                    max_restarts: 5,
                    restart_delay: Duration::from_secs(5),
                },
            ]);
        }
        Ok(configs)
    }

    /// Spawn a single agent process
    pub async fn spawn_agent(&mut self, config: AgentConfig) -> Result<()> {
        let name = config.name.clone();
        info!("Spawning agent: {} (module: {})", name, config.module);

        let child = tokio::process::Command::new(&self.python_path)
            .arg("-m")
            .arg(&config.module)
            .env("AIOS_AGENT_NAME", &config.name)
            .env("AIOS_AGENT_TYPE", &config.agent_type)
            .env("AIOS_ORCHESTRATOR_ADDR", "127.0.0.1:50051")
            .kill_on_drop(true)
            .spawn();

        let process = match child {
            Ok(p) => Some(p),
            Err(e) => {
                warn!("Failed to spawn agent {}: {}", name, e);
                None
            }
        };

        let status = if process.is_some() {
            AgentProcessStatus::Running
        } else {
            AgentProcessStatus::Failed
        };

        self.agents.insert(
            name,
            SpawnedAgent {
                config,
                process,
                restart_count: 0,
                last_heartbeat: Instant::now(),
                started_at: Instant::now(),
                status,
            },
        );

        Ok(())
    }

    /// Check all agents and restart any that have died
    pub async fn check_and_restart(&mut self) {
        let mut to_restart = Vec::new();

        for (name, agent) in &mut self.agents {
            // Check if process is still running
            if let Some(ref mut child) = agent.process {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        warn!("Agent {} exited with status: {}", name, status);
                        agent.status = AgentProcessStatus::Failed;
                        agent.process = None;
                        if agent.restart_count < agent.config.max_restarts {
                            to_restart.push(agent.config.clone());
                        } else {
                            error!(
                                "Agent {} exceeded max restarts ({}), not restarting",
                                name, agent.config.max_restarts
                            );
                        }
                    }
                    Ok(None) => {
                        // Still running, check heartbeat
                        if agent.last_heartbeat.elapsed() > self.heartbeat_timeout {
                            warn!("Agent {} heartbeat timeout", name);
                        }
                    }
                    Err(e) => {
                        warn!("Error checking agent {} status: {}", name, e);
                    }
                }
            }
        }

        // Restart failed agents
        for config in to_restart {
            let name = config.name.clone();
            let delay = config.restart_delay;
            if let Some(agent) = self.agents.get_mut(&name) {
                agent.restart_count += 1;
                info!(
                    "Restarting agent {} (attempt {}/{})",
                    name, agent.restart_count, config.max_restarts
                );
            }
            tokio::time::sleep(delay).await;
            if let Err(e) = self.spawn_agent(config).await {
                error!("Failed to restart agent {}: {}", name, e);
            }
        }
    }

    /// Update heartbeat for an agent
    pub fn update_heartbeat(&mut self, agent_name: &str) {
        if let Some(agent) = self.agents.get_mut(agent_name) {
            agent.last_heartbeat = Instant::now();
            agent.status = AgentProcessStatus::Running;
        }
    }

    /// Stop all agent processes
    pub async fn stop_all(&mut self) {
        info!("Stopping all agent processes...");
        for (name, agent) in &mut self.agents {
            if let Some(ref mut child) = agent.process {
                info!("Sending SIGTERM to agent {}", name);
                let _ = child.kill().await;
                agent.status = AgentProcessStatus::Stopped;
            }
        }
    }

    /// Get status of all agents
    pub fn get_status(&self) -> Vec<(String, AgentProcessStatus, u32)> {
        self.agents
            .iter()
            .map(|(name, agent)| {
                (name.clone(), agent.status.clone(), agent.restart_count)
            })
            .collect()
    }

    /// Run the agent monitor loop
    pub async fn run_monitor(
        spawner: std::sync::Arc<tokio::sync::RwLock<Self>>,
        cancel: CancellationToken,
    ) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Agent spawner monitor shutting down");
                    spawner.write().await.stop_all().await;
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    spawner.write().await.check_and_restart().await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawner_new() {
        let spawner = AgentSpawner::new("/etc/aios/agents");
        assert!(spawner.agents.is_empty());
    }

    #[test]
    fn test_load_configs() {
        let mut spawner = AgentSpawner::new("/nonexistent");
        let configs = spawner.load_configs().unwrap();
        assert_eq!(configs.len(), 3);
        assert_eq!(configs[0].name, "system-agent");
        assert_eq!(configs[1].name, "network-agent");
        assert_eq!(configs[2].name, "security-agent");
    }

    #[test]
    fn test_get_status_empty() {
        let spawner = AgentSpawner::new("/etc/aios/agents");
        assert!(spawner.get_status().is_empty());
    }

    #[test]
    fn test_update_heartbeat() {
        let mut spawner = AgentSpawner::new("/etc/aios/agents");
        spawner.agents.insert(
            "test-agent".to_string(),
            SpawnedAgent {
                config: AgentConfig {
                    name: "test-agent".to_string(),
                    agent_type: "test".to_string(),
                    module: "test".to_string(),
                    capabilities: vec![],
                    tool_namespaces: vec![],
                    max_restarts: 3,
                    restart_delay: Duration::from_secs(1),
                },
                process: None,
                restart_count: 0,
                last_heartbeat: Instant::now() - Duration::from_secs(60),
                started_at: Instant::now(),
                status: AgentProcessStatus::Running,
            },
        );

        spawner.update_heartbeat("test-agent");
        let agent = spawner.agents.get("test-agent").unwrap();
        assert!(agent.last_heartbeat.elapsed() < Duration::from_secs(1));
    }
}
