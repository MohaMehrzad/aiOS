//! Inter-Service Health Checker
//!
//! Periodically pings all aiOS services (runtime, tools, memory, api-gateway)
//! and reports their health status.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

/// Health status for a single service
#[derive(Debug, Clone)]
pub struct ServiceHealthStatus {
    pub name: String,
    pub address: SocketAddr,
    pub healthy: bool,
    pub last_check_ms: u64,
    pub last_checked_at: i64,
    pub consecutive_failures: u32,
}

/// Tracks health of all inter-service dependencies
pub struct HealthChecker {
    services: HashMap<String, ServiceHealthStatus>,
    check_interval: Duration,
    timeout: Duration,
}

impl HealthChecker {
    pub fn new() -> Self {
        let mut services = HashMap::new();
        let default_services = [
            ("runtime", "127.0.0.1:50055"),
            ("tools", "127.0.0.1:50052"),
            ("memory", "127.0.0.1:50053"),
            ("api-gateway", "127.0.0.1:50054"),
        ];

        for (name, addr) in &default_services {
            let address: SocketAddr = addr.parse().expect("valid socket addr");
            services.insert(
                name.to_string(),
                ServiceHealthStatus {
                    name: name.to_string(),
                    address,
                    healthy: false,
                    last_check_ms: 0,
                    last_checked_at: 0,
                    consecutive_failures: 0,
                },
            );
        }

        Self {
            services,
            check_interval: Duration::from_secs(10),
            timeout: Duration::from_secs(2),
        }
    }

    /// Check health of all services via TCP connect
    pub async fn check_all(&mut self) {
        let now = chrono::Utc::now().timestamp();

        for status in self.services.values_mut() {
            let start = std::time::Instant::now();
            let healthy =
                tokio::time::timeout(self.timeout, TcpStream::connect(status.address))
                    .await
                    .map(|r| r.is_ok())
                    .unwrap_or(false);

            status.last_check_ms = start.elapsed().as_millis() as u64;
            status.last_checked_at = now;

            if healthy {
                if !status.healthy {
                    debug!("Service {} is now healthy", status.name);
                }
                status.healthy = true;
                status.consecutive_failures = 0;
            } else {
                status.consecutive_failures += 1;
                status.healthy = false;
                if status.consecutive_failures <= 3 {
                    warn!(
                        "Service {} health check failed (attempt {})",
                        status.name, status.consecutive_failures
                    );
                }
            }
        }
    }

    /// Get current health status of all services
    pub fn get_all_status(&self) -> Vec<ServiceHealthStatus> {
        self.services.values().cloned().collect()
    }

    /// Check if all services are healthy
    pub fn all_healthy(&self) -> bool {
        self.services.values().all(|s| s.healthy)
    }

    /// Start the health check background loop
    pub async fn run(checker: Arc<RwLock<Self>>, cancel: CancellationToken) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("Health checker shutting down");
                    break;
                }
                _ = tokio::time::sleep(checker.read().await.check_interval) => {
                    checker.write().await.check_all().await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker_new() {
        let checker = HealthChecker::new();
        assert_eq!(checker.services.len(), 4);
        assert!(!checker.all_healthy());
    }

    #[test]
    fn test_get_all_status() {
        let checker = HealthChecker::new();
        let statuses = checker.get_all_status();
        assert_eq!(statuses.len(), 4);
        for status in &statuses {
            assert!(!status.healthy);
            assert_eq!(status.consecutive_failures, 0);
        }
    }

    #[tokio::test]
    async fn test_check_all_no_services() {
        let mut checker = HealthChecker::new();
        checker.timeout = Duration::from_millis(50);
        checker.check_all().await;
        // All should fail since no services are running
        assert!(!checker.all_healthy());
        for status in checker.services.values() {
            assert!(!status.healthy);
            assert_eq!(status.consecutive_failures, 1);
        }
    }
}
