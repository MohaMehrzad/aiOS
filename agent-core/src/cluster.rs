//! Multi-Node Cluster Management
//!
//! Handles node registration, heartbeats, and cluster state tracking.
//! Enables cross-node task routing and distributed operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Information about a cluster node
#[derive(Debug, Clone)]
pub struct ClusterNode {
    pub node_id: String,
    pub hostname: String,
    pub address: String,
    pub agents: Vec<String>,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub active_tasks: u32,
    pub max_tasks: u32,
    pub last_heartbeat: Instant,
    pub registered_at: Instant,
    pub metadata: HashMap<String, String>,
}

/// Cluster manager
pub struct ClusterManager {
    nodes: HashMap<String, ClusterNode>,
    local_node_id: String,
    heartbeat_timeout_secs: u64,
    enabled: bool,
}

impl ClusterManager {
    pub fn new(local_node_id: &str) -> Self {
        Self {
            nodes: HashMap::new(),
            local_node_id: local_node_id.to_string(),
            heartbeat_timeout_secs: 30,
            enabled: std::env::var("AIOS_CLUSTER_ENABLED").unwrap_or_default() == "true",
        }
    }

    /// Check if cluster mode is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Register a remote node
    pub fn register_node(&mut self, node: ClusterNode) {
        info!(
            "Cluster node registered: {} ({}) with {} agents",
            node.node_id,
            node.hostname,
            node.agents.len()
        );
        self.nodes.insert(node.node_id.clone(), node);
    }

    /// Update node heartbeat
    pub fn node_heartbeat(
        &mut self,
        node_id: &str,
        cpu_usage: f64,
        memory_usage: f64,
        active_tasks: u32,
    ) {
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.last_heartbeat = Instant::now();
            node.cpu_usage = cpu_usage;
            node.memory_usage = memory_usage;
            node.active_tasks = active_tasks;
            debug!("Cluster heartbeat from {node_id}: cpu={cpu_usage:.1}%, tasks={active_tasks}");
        }
    }

    /// Remove a node
    pub fn remove_node(&mut self, node_id: &str) {
        if self.nodes.remove(node_id).is_some() {
            info!("Cluster node removed: {node_id}");
        }
    }

    /// List all healthy nodes
    pub fn list_healthy_nodes(&self) -> Vec<&ClusterNode> {
        self.nodes
            .values()
            .filter(|n| n.last_heartbeat.elapsed().as_secs() < self.heartbeat_timeout_secs)
            .collect()
    }

    /// List all nodes (including stale)
    pub fn list_all_nodes(&self) -> Vec<&ClusterNode> {
        self.nodes.values().collect()
    }

    /// Find dead nodes
    pub fn dead_nodes(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.last_heartbeat.elapsed().as_secs() >= self.heartbeat_timeout_secs)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Find the best node for a task based on required agents and load
    pub fn route_to_node(&self, required_agent_type: &str) -> Option<&ClusterNode> {
        self.nodes
            .values()
            .filter(|n| {
                n.last_heartbeat.elapsed().as_secs() < self.heartbeat_timeout_secs
                    && n.active_tasks < n.max_tasks
                    && (required_agent_type.is_empty()
                        || n.agents.iter().any(|a| a.contains(required_agent_type)))
            })
            .min_by(|a, b| {
                let a_load =
                    a.cpu_usage + (a.active_tasks as f64 / a.max_tasks.max(1) as f64) * 100.0;
                let b_load =
                    b.cpu_usage + (b.active_tasks as f64 / b.max_tasks.max(1) as f64) * 100.0;
                a_load
                    .partial_cmp(&b_load)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Get the local node ID
    pub fn local_node_id(&self) -> &str {
        &self.local_node_id
    }

    /// Run cluster health monitoring loop
    pub async fn run_monitor(cluster: Arc<RwLock<Self>>, cancel: CancellationToken) {
        info!("Cluster monitor started");
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Cluster monitor shutting down");
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                    let mut cm = cluster.write().await;
                    let dead = cm.dead_nodes();
                    for node_id in &dead {
                        warn!("Cluster node {node_id} is dead (no heartbeat)");
                        cm.remove_node(node_id);
                    }
                    let healthy_count = cm.list_healthy_nodes().len();
                    if healthy_count > 0 {
                        debug!("Cluster status: {} healthy nodes", healthy_count);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, agents: Vec<&str>) -> ClusterNode {
        ClusterNode {
            node_id: id.to_string(),
            hostname: format!("{id}.local"),
            address: "http://127.0.0.1:50051".to_string(),
            agents: agents.into_iter().map(|s| s.to_string()).collect(),
            cpu_usage: 30.0,
            memory_usage: 50.0,
            active_tasks: 2,
            max_tasks: 10,
            last_heartbeat: Instant::now(),
            registered_at: Instant::now(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_cluster_manager_new() {
        let cm = ClusterManager::new("node-1");
        assert_eq!(cm.local_node_id(), "node-1");
        assert!(cm.list_all_nodes().is_empty());
    }

    #[test]
    fn test_register_and_list() {
        let mut cm = ClusterManager::new("local");
        cm.register_node(make_node("remote-1", vec!["system", "network"]));
        assert_eq!(cm.list_all_nodes().len(), 1);
        assert_eq!(cm.list_healthy_nodes().len(), 1);
    }

    #[test]
    fn test_route_to_node() {
        let mut cm = ClusterManager::new("local");
        cm.register_node(make_node("node-1", vec!["system"]));
        cm.register_node(make_node("node-2", vec!["network"]));

        let node = cm.route_to_node("network");
        assert!(node.is_some());
        assert_eq!(node.unwrap().node_id, "node-2");
    }

    #[test]
    fn test_remove_node() {
        let mut cm = ClusterManager::new("local");
        cm.register_node(make_node("remote-1", vec!["system"]));
        cm.remove_node("remote-1");
        assert!(cm.list_all_nodes().is_empty());
    }
}
