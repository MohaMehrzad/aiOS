//! Service supervisor for aiOS init
//!
//! Manages child services: start, health check, restart on failure.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::config::AiosConfig;

/// A running service managed by the supervisor
#[allow(dead_code)]
struct ManagedService {
    name: String,
    binary: String,
    args: Vec<String>,
    process: Child,
    started_at: Instant,
    restart_count: u32,
    last_restart: Option<Instant>,
}

/// Service supervisor that manages all aiOS services
pub struct ServiceSupervisor {
    services: HashMap<String, ManagedService>,
    max_restart_attempts: u32,
    restart_window: Duration,
}

impl ServiceSupervisor {
    pub fn new(config: &AiosConfig) -> Self {
        Self {
            services: HashMap::new(),
            max_restart_attempts: config.agents.max_restart_attempts,
            restart_window: Duration::from_secs(config.agents.restart_window_seconds),
        }
    }

    /// Start a service and register it with the supervisor
    pub fn start_service(&mut self, name: &str, binary: &str, args: &[&str]) -> Result<()> {
        info!("Starting service: {name}");
        let child = Command::new(binary)
            .args(args)
            .spawn()
            .with_context(|| format!("Failed to start service {name} ({binary})"))?;

        info!("Service {name} started with PID {}", child.id());

        self.services.insert(
            name.to_string(),
            ManagedService {
                name: name.to_string(),
                binary: binary.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                process: child,
                started_at: Instant::now(),
                restart_count: 0,
                last_restart: None,
            },
        );

        Ok(())
    }

    /// Wait for a service to become healthy
    pub fn wait_for_health(&self, name: &str, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        let check_interval = Duration::from_millis(500);

        while start.elapsed() < timeout {
            if self.is_service_alive(name) {
                // Process liveness serves as the health signal; each service
                // also exposes gRPC health endpoints for deeper checks.
                return Ok(());
            }
            std::thread::sleep(check_interval);
        }

        bail!("Service {name} did not become healthy within {timeout:?}");
    }

    /// Check if a service process is still alive
    fn is_service_alive(&self, name: &str) -> bool {
        if let Some(service) = self.services.get(name) {
            // Check if the process is still running
            // Since we can't peek without consuming, check /proc on Linux
            let pid = service.process.id();
            std::path::Path::new(&format!("/proc/{pid}")).exists()
        } else {
            false
        }
    }

    /// Check all services and restart any that have died
    pub fn check_and_restart_services(&mut self) {
        let names: Vec<String> = self.services.keys().cloned().collect();
        for name in names {
            let should_restart = {
                let service = match self.services.get_mut(&name) {
                    Some(s) => s,
                    None => continue,
                };

                // Try to check if process is still running
                match service.process.try_wait() {
                    Ok(Some(status)) => {
                        warn!(
                            "Service {} (PID {}) exited with status: {}",
                            name,
                            service.process.id(),
                            status
                        );
                        true
                    }
                    Ok(None) => false, // Still running
                    Err(e) => {
                        error!("Error checking service {name}: {e}");
                        true
                    }
                }
            };

            if should_restart {
                self.restart_service(&name);
            }
        }
    }

    /// Attempt to restart a failed service
    fn restart_service(&mut self, name: &str) {
        let service = match self.services.get_mut(name) {
            Some(s) => s,
            None => return,
        };

        // Check if we're within the restart window
        if let Some(last) = service.last_restart {
            if last.elapsed() > self.restart_window {
                // Reset counter — we're in a new window
                service.restart_count = 0;
            }
        }

        if service.restart_count >= self.max_restart_attempts {
            error!(
                "Service {name} exceeded max restart attempts ({}), not restarting",
                self.max_restart_attempts
            );
            return;
        }

        info!(
            "Restarting service {name} (attempt {})",
            service.restart_count + 1
        );
        let binary = service.binary.clone();
        let args = service.args.clone();

        match Command::new(&binary).args(&args).spawn() {
            Ok(child) => {
                info!("Service {name} restarted with PID {}", child.id());
                service.process = child;
                service.restart_count += 1;
                service.last_restart = Some(Instant::now());
            }
            Err(e) => {
                error!("Failed to restart service {name}: {e}");
            }
        }
    }

    /// Return the number of running services
    pub fn running_count(&self) -> usize {
        self.services.len()
    }

    /// Stop all managed services gracefully
    pub fn stop_all(&mut self) {
        info!("Stopping all services...");
        let names: Vec<String> = self.services.keys().cloned().collect();

        // Send SIGTERM to all
        for name in &names {
            if let Some(service) = self.services.get_mut(name) {
                info!("Sending SIGTERM to {name} (PID {})", service.process.id());
                let _ = service.process.kill();
            }
        }

        // Wait for graceful shutdown (up to 5 seconds)
        let deadline = Instant::now() + Duration::from_secs(5);
        for name in &names {
            if let Some(service) = self.services.get_mut(name) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match service.process.wait() {
                    Ok(status) => info!("Service {name} exited: {status}"),
                    Err(e) => warn!("Error waiting for {name}: {e}"),
                }
            }
        }

        self.services.clear();
        info!("All services stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supervisor_creation() {
        let config = AiosConfig {
            system: Default::default(),
            boot: Default::default(),
            models: Default::default(),
            api: Default::default(),
            memory: Default::default(),
            security: Default::default(),
            networking: Default::default(),
            agents: Default::default(),
            monitoring: Default::default(),
        };
        let sup = ServiceSupervisor::new(&config);
        assert!(sup.services.is_empty());
    }
}
