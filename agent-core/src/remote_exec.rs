//! Remote Execution Client
//!
//! Forwards tool execution requests and goal submissions to remote
//! cluster nodes via gRPC.

use anyhow::{Context, Result};
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, info};

/// Client for executing operations on remote aiOS nodes
pub struct RemoteExecutor {
    channels: std::collections::HashMap<String, Channel>,
}

impl RemoteExecutor {
    pub fn new() -> Self {
        Self {
            channels: std::collections::HashMap::new(),
        }
    }

    /// Get or create a channel to a remote node
    async fn get_channel(&mut self, address: &str) -> Result<Channel> {
        if let Some(channel) = self.channels.get(address) {
            return Ok(channel.clone());
        }

        let endpoint = Endpoint::from_shared(address.to_string())
            .context("Invalid remote address")?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60));

        let channel = endpoint
            .connect()
            .await
            .context("Failed to connect to remote node")?;

        info!("Connected to remote node at {address}");
        self.channels.insert(address.to_string(), channel.clone());
        Ok(channel)
    }

    /// Submit a goal to a remote orchestrator
    pub async fn submit_remote_goal(
        &mut self,
        address: &str,
        description: &str,
        priority: i32,
        source: &str,
    ) -> Result<String> {
        let channel = self.get_channel(address).await?;
        let mut client =
            crate::proto::orchestrator::orchestrator_client::OrchestratorClient::new(channel);

        let request = tonic::Request::new(crate::proto::orchestrator::SubmitGoalRequest {
            description: description.to_string(),
            priority,
            source: source.to_string(),
            tags: vec![],
            metadata_json: vec![],
        });

        let response = client
            .submit_goal(request)
            .await
            .context("Remote goal submission failed")?;

        let goal_id = response.into_inner().id;
        info!("Submitted goal to remote node {address}: {goal_id}");
        Ok(goal_id)
    }

    /// Execute a tool on a remote node's tool service
    pub async fn execute_remote_tool(
        &mut self,
        tools_address: &str,
        tool_name: &str,
        agent_id: &str,
        task_id: &str,
        input_json: &[u8],
    ) -> Result<(bool, Vec<u8>, String)> {
        let channel = self.get_channel(tools_address).await?;
        let mut client =
            crate::proto::tools::tool_registry_client::ToolRegistryClient::new(channel);

        let request = tonic::Request::new(crate::proto::tools::ExecuteRequest {
            tool_name: tool_name.to_string(),
            agent_id: agent_id.to_string(),
            task_id: task_id.to_string(),
            input_json: input_json.to_vec(),
            reason: "Remote execution from cluster".to_string(),
        });

        let response = client
            .execute(request)
            .await
            .context("Remote tool execution failed")?;

        let resp = response.into_inner();
        Ok((resp.success, resp.output_json, resp.error))
    }

    /// Close all cached channels
    pub fn close_all(&mut self) {
        self.channels.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_executor_new() {
        let exec = RemoteExecutor::new();
        assert!(exec.channels.is_empty());
    }
}
