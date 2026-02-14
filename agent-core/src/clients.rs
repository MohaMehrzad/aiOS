//! Inter-Service gRPC Clients
//!
//! Provides lazy-connecting gRPC client stubs for all aiOS services:
//! runtime, tools, memory, and api-gateway.

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OnceCell, RwLock};
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, info, warn};

use crate::discovery::ServiceRegistry;
use crate::proto;

/// Holds gRPC client connections to all aiOS services
pub struct ServiceClients {
    runtime_channel: OnceCell<Channel>,
    tools_channel: OnceCell<Channel>,
    memory_channel: OnceCell<Channel>,
    api_gateway_channel: OnceCell<Channel>,
    runtime_addr: String,
    tools_addr: String,
    memory_addr: String,
    api_gateway_addr: String,
    /// Optional service discovery registry for dynamic address resolution
    discovery: Option<Arc<RwLock<ServiceRegistry>>>,
}

impl ServiceClients {
    pub fn new() -> Self {
        Self {
            runtime_channel: OnceCell::new(),
            tools_channel: OnceCell::new(),
            memory_channel: OnceCell::new(),
            api_gateway_channel: OnceCell::new(),
            runtime_addr: std::env::var("AIOS_RUNTIME_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:50055".to_string()),
            tools_addr: std::env::var("AIOS_TOOLS_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:50052".to_string()),
            memory_addr: std::env::var("AIOS_MEMORY_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:50053".to_string()),
            api_gateway_addr: std::env::var("AIOS_GATEWAY_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:50054".to_string()),
            discovery: None,
        }
    }

    /// Create clients with service discovery support
    pub fn with_discovery(discovery: Arc<RwLock<ServiceRegistry>>) -> Self {
        let mut clients = Self::new();
        clients.discovery = Some(discovery);
        clients
    }

    /// Resolve a service address via discovery, falling back to the hardcoded default
    async fn resolve_addr(&self, service_name: &str, default: &str) -> String {
        if std::env::var("AIOS_USE_DISCOVERY").unwrap_or_default() != "true" {
            return default.to_string();
        }
        if let Some(ref registry) = self.discovery {
            let reg = registry.read().await;
            if let Some(info) = reg.lookup(service_name) {
                let addr = format!("http://{}", info.address);
                debug!("Resolved {service_name} via discovery: {addr}");
                return addr;
            }
        }
        default.to_string()
    }

    /// Create a channel with retry logic
    async fn connect_with_retry(addr: &str) -> Result<Channel> {
        let endpoint = Endpoint::from_shared(addr.to_string())
            .context("Invalid endpoint address")?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(300))
            .tcp_keepalive(Some(Duration::from_secs(10)));

        for attempt in 1..=3 {
            match endpoint.connect().await {
                Ok(channel) => {
                    info!("Connected to {addr} (attempt {attempt})");
                    return Ok(channel);
                }
                Err(e) => {
                    warn!("Connection to {addr} failed (attempt {attempt}): {e}");
                    if attempt < 3 {
                        tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
                    }
                }
            }
        }

        // Return a lazy channel that will connect on first use
        Ok(endpoint.connect_lazy())
    }

    /// Get or create the runtime gRPC client
    pub async fn runtime(
        &self,
    ) -> Result<proto::runtime::ai_runtime_client::AiRuntimeClient<Channel>> {
        let channel = self
            .runtime_channel
            .get_or_try_init(|| Self::connect_with_retry(&self.runtime_addr))
            .await?;
        Ok(proto::runtime::ai_runtime_client::AiRuntimeClient::new(
            channel.clone(),
        ))
    }

    /// Get or create the tools gRPC client
    pub async fn tools(
        &self,
    ) -> Result<proto::tools::tool_registry_client::ToolRegistryClient<Channel>> {
        let channel = self
            .tools_channel
            .get_or_try_init(|| Self::connect_with_retry(&self.tools_addr))
            .await?;
        Ok(proto::tools::tool_registry_client::ToolRegistryClient::new(
            channel.clone(),
        ))
    }

    /// Get or create the memory gRPC client
    pub async fn memory(
        &self,
    ) -> Result<proto::memory::memory_service_client::MemoryServiceClient<Channel>> {
        let channel = self
            .memory_channel
            .get_or_try_init(|| Self::connect_with_retry(&self.memory_addr))
            .await?;
        Ok(proto::memory::memory_service_client::MemoryServiceClient::new(channel.clone()))
    }

    /// Get or create the api-gateway gRPC client
    pub async fn api_gateway(
        &self,
    ) -> Result<proto::api_gateway::api_gateway_client::ApiGatewayClient<Channel>> {
        let channel = self
            .api_gateway_channel
            .get_or_try_init(|| Self::connect_with_retry(&self.api_gateway_addr))
            .await?;
        Ok(proto::api_gateway::api_gateway_client::ApiGatewayClient::new(channel.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_clients_new() {
        let clients = ServiceClients::new();
        assert_eq!(clients.runtime_addr, "http://127.0.0.1:50055");
        assert_eq!(clients.tools_addr, "http://127.0.0.1:50052");
        assert_eq!(clients.memory_addr, "http://127.0.0.1:50053");
        assert_eq!(clients.api_gateway_addr, "http://127.0.0.1:50054");
    }
}
