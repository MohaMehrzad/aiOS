//! Service Discovery — simple registry for aiOS services
//!
//! Services register on startup with their address and capabilities.
//! Clients look up service addresses instead of hardcoding ports.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Information about a registered service
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub address: SocketAddr,
    pub service_type: String,
    pub version: String,
    pub registered_at: Instant,
    pub last_heartbeat: Instant,
    pub metadata: HashMap<String, String>,
}

/// Service discovery registry
pub struct ServiceRegistry {
    services: HashMap<String, ServiceInfo>,
    heartbeat_timeout: Duration,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            heartbeat_timeout: Duration::from_secs(30),
        }
    }

    /// Register a service
    pub fn register(&mut self, name: &str, address: SocketAddr, service_type: &str, version: &str) {
        info!("Service registered: {name} at {address} (type: {service_type})");

        self.services.insert(
            name.to_string(),
            ServiceInfo {
                name: name.to_string(),
                address,
                service_type: service_type.to_string(),
                version: version.to_string(),
                registered_at: Instant::now(),
                last_heartbeat: Instant::now(),
                metadata: HashMap::new(),
            },
        );
    }

    /// Register with default aiOS service ports
    pub fn register_defaults(&mut self) {
        let defaults = [
            (
                "orchestrator",
                "0.0.0.0:50051",
                "grpc",
                env!("CARGO_PKG_VERSION"),
            ),
            ("runtime", "0.0.0.0:50055", "grpc", "0.1.0"),
            ("tools", "0.0.0.0:50052", "grpc", "0.1.0"),
            ("memory", "0.0.0.0:50053", "grpc", "0.1.0"),
            ("api-gateway", "0.0.0.0:50054", "grpc", "0.1.0"),
            (
                "management",
                "0.0.0.0:9090",
                "http",
                env!("CARGO_PKG_VERSION"),
            ),
        ];

        for (name, addr, svc_type, version) in &defaults {
            let address: SocketAddr = addr.parse().expect("valid default address");
            self.register(name, address, svc_type, version);
        }
    }

    /// Deregister a service
    pub fn deregister(&mut self, name: &str) {
        if self.services.remove(name).is_some() {
            info!("Service deregistered: {name}");
        }
    }

    /// Look up a service by name
    pub fn lookup(&self, name: &str) -> Option<&ServiceInfo> {
        self.services
            .get(name)
            .filter(|s| s.last_heartbeat.elapsed() < self.heartbeat_timeout)
    }

    /// Look up services by type
    pub fn lookup_by_type(&self, service_type: &str) -> Vec<&ServiceInfo> {
        self.services
            .values()
            .filter(|s| {
                s.service_type == service_type
                    && s.last_heartbeat.elapsed() < self.heartbeat_timeout
            })
            .collect()
    }

    /// Update heartbeat for a service
    pub fn heartbeat(&mut self, name: &str) {
        if let Some(service) = self.services.get_mut(name) {
            service.last_heartbeat = Instant::now();
            debug!("Heartbeat received from {name}");
        }
    }

    /// Get all registered services
    pub fn list_all(&self) -> Vec<&ServiceInfo> {
        self.services.values().collect()
    }

    /// Get all healthy (non-timed-out) services
    pub fn list_healthy(&self) -> Vec<&ServiceInfo> {
        self.services
            .values()
            .filter(|s| s.last_heartbeat.elapsed() < self.heartbeat_timeout)
            .collect()
    }

    /// Prune timed-out services
    pub fn prune_stale(&mut self) -> Vec<String> {
        let stale: Vec<String> = self
            .services
            .iter()
            .filter(|(_, s)| s.last_heartbeat.elapsed() >= self.heartbeat_timeout)
            .map(|(name, _)| name.clone())
            .collect();

        for name in &stale {
            warn!("Pruning stale service: {name}");
            self.services.remove(name);
        }

        stale
    }

    /// Run discovery service background loop
    pub async fn run(registry: Arc<RwLock<Self>>, cancel: tokio_util::sync::CancellationToken) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Service discovery shutting down");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(15)) => {
                    let pruned = registry.write().await.prune_stale();
                    if !pruned.is_empty() {
                        warn!("Pruned {} stale services", pruned.len());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_lookup() {
        let mut registry = ServiceRegistry::new();
        let addr: SocketAddr = "127.0.0.1:50051".parse().unwrap();
        registry.register("orchestrator", addr, "grpc", "0.1.0");

        let svc = registry.lookup("orchestrator").unwrap();
        assert_eq!(svc.address, addr);
        assert_eq!(svc.service_type, "grpc");
    }

    #[test]
    fn test_lookup_nonexistent() {
        let registry = ServiceRegistry::new();
        assert!(registry.lookup("nonexistent").is_none());
    }

    #[test]
    fn test_deregister() {
        let mut registry = ServiceRegistry::new();
        let addr: SocketAddr = "127.0.0.1:50051".parse().unwrap();
        registry.register("svc", addr, "grpc", "0.1.0");
        registry.deregister("svc");
        assert!(registry.lookup("svc").is_none());
    }

    #[test]
    fn test_register_defaults() {
        let mut registry = ServiceRegistry::new();
        registry.register_defaults();
        assert_eq!(registry.list_all().len(), 6);
        assert!(registry.lookup("orchestrator").is_some());
        assert!(registry.lookup("memory").is_some());
    }

    #[test]
    fn test_lookup_by_type() {
        let mut registry = ServiceRegistry::new();
        registry.register_defaults();

        let grpc_services = registry.lookup_by_type("grpc");
        assert_eq!(grpc_services.len(), 5);

        let http_services = registry.lookup_by_type("http");
        assert_eq!(http_services.len(), 1);
    }

    #[test]
    fn test_heartbeat() {
        let mut registry = ServiceRegistry::new();
        let addr: SocketAddr = "127.0.0.1:50051".parse().unwrap();
        registry.register("svc", addr, "grpc", "0.1.0");
        registry.heartbeat("svc");

        let svc = registry.lookup("svc").unwrap();
        assert!(svc.last_heartbeat.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_list_healthy() {
        let mut registry = ServiceRegistry::new();
        let addr: SocketAddr = "127.0.0.1:50051".parse().unwrap();
        registry.register("svc", addr, "grpc", "0.1.0");
        assert_eq!(registry.list_healthy().len(), 1);
    }
}
