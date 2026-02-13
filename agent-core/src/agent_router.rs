//! Agent Router — manages agent registry and task routing
//!
//! Maps task requirements to available agents based on capabilities,
//! load, and health status.

use std::collections::HashMap;
use std::time::Instant;
use tracing::info;

use crate::proto::common::{AgentRegistration, Task};

/// Agent state tracked by the router
struct TrackedAgent {
    registration: AgentRegistration,
    last_heartbeat: Instant,
    status: String,
    current_task: Option<String>,
    tasks_completed: u32,
    tasks_failed: u32,
}

/// Routes tasks to the most appropriate agent
pub struct AgentRouter {
    agents: HashMap<String, TrackedAgent>,
    heartbeat_timeout_secs: u64,
}

impl AgentRouter {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            heartbeat_timeout_secs: 15,
        }
    }

    /// Register a new agent
    pub async fn register_agent(&mut self, registration: AgentRegistration) {
        let agent_id = registration.agent_id.clone();
        info!(
            "Registering agent: {} (type: {}, capabilities: {:?})",
            agent_id, registration.agent_type, registration.capabilities
        );

        self.agents.insert(
            agent_id,
            TrackedAgent {
                registration,
                last_heartbeat: Instant::now(),
                status: "idle".to_string(),
                current_task: None,
                tasks_completed: 0,
                tasks_failed: 0,
            },
        );
    }

    /// Unregister an agent
    pub async fn unregister_agent(&mut self, agent_id: &str) {
        if self.agents.remove(agent_id).is_some() {
            info!("Unregistered agent: {agent_id}");
        }
    }

    /// Update heartbeat for an agent
    pub fn update_heartbeat(&mut self, agent_id: &str, status: &str) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.last_heartbeat = Instant::now();
            agent.status = status.to_string();
        }
    }

    /// Find the best agent for a task
    pub fn route_task(&self, task: &Task) -> Option<String> {
        let required_tools = &task.required_tools;

        // Find agents that:
        // 1. Have matching capabilities (tool namespaces)
        // 2. Are healthy (recent heartbeat)
        // 3. Are idle (no current task)
        let mut candidates: Vec<(&String, &TrackedAgent)> = self
            .agents
            .iter()
            .filter(|(_, agent)| {
                // Check health
                agent.last_heartbeat.elapsed().as_secs() < self.heartbeat_timeout_secs
            })
            .filter(|(_, agent)| {
                // Check availability
                agent.status == "idle" && agent.current_task.is_none()
            })
            .filter(|(_, agent)| {
                // Check capabilities match — tasks with no required_tools
                // go to AI inference, not agents
                if required_tools.is_empty() {
                    return false;
                }
                required_tools.iter().any(|tool| {
                    agent.registration.tool_namespaces.contains(tool)
                        || agent
                            .registration
                            .capabilities
                            .iter()
                            .any(|cap| cap.contains(tool))
                })
            })
            .collect();

        if candidates.is_empty() {
            // Try agents that are busy but capable (queue the task)
            candidates = self
                .agents
                .iter()
                .filter(|(_, agent)| {
                    agent.last_heartbeat.elapsed().as_secs() < self.heartbeat_timeout_secs
                })
                .filter(|(_, agent)| {
                    if required_tools.is_empty() {
                        return false;
                    }
                    required_tools.iter().any(|tool| {
                        agent.registration.tool_namespaces.contains(tool)
                    })
                })
                .collect();
        }

        // Sort by: idle first, then by completed task count (prefer experienced agents)
        candidates.sort_by(|a, b| {
            let a_idle = a.1.status == "idle";
            let b_idle = b.1.status == "idle";
            b_idle
                .cmp(&a_idle)
                .then(b.1.tasks_completed.cmp(&a.1.tasks_completed))
        });

        candidates.first().map(|(id, _)| (*id).clone())
    }

    /// Assign a task to an agent
    pub fn assign_task(&mut self, agent_id: &str, task_id: &str) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.current_task = Some(task_id.to_string());
            agent.status = "busy".to_string();
        }
    }

    /// Mark a task as completed by an agent
    pub fn task_completed(&mut self, agent_id: &str, success: bool) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.current_task = None;
            agent.status = "idle".to_string();
            if success {
                agent.tasks_completed += 1;
            } else {
                agent.tasks_failed += 1;
            }
        }
    }

    /// List all registered agents
    pub async fn list_agents(&self) -> Vec<AgentRegistration> {
        self.agents
            .values()
            .map(|a| {
                let mut reg = a.registration.clone();
                reg.status = a.status.clone();
                reg
            })
            .collect()
    }

    /// Get count of active (healthy) agents
    pub fn active_agent_count(&self) -> usize {
        self.agents
            .values()
            .filter(|a| a.last_heartbeat.elapsed().as_secs() < self.heartbeat_timeout_secs)
            .count()
    }

    /// Get the task assigned to a specific agent (if any)
    pub fn get_assigned_task_id(&self, agent_id: &str) -> Option<String> {
        self.agents
            .get(agent_id)
            .and_then(|a| a.current_task.clone())
    }

    /// Get agents that have timed out
    pub fn dead_agents(&self) -> Vec<String> {
        self.agents
            .iter()
            .filter(|(_, a)| a.last_heartbeat.elapsed().as_secs() >= self.heartbeat_timeout_secs)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Route a task to a remote cluster node if no local agent can handle it.
    /// Returns (node_id, agent_type) if a remote node has a suitable agent.
    pub fn route_task_to_node(
        &self,
        task: &Task,
        cluster: &crate::cluster::ClusterManager,
    ) -> Option<String> {
        // Only try remote routing if local routing fails
        if self.route_task(task).is_some() {
            return None;
        }

        // Extract required agent type from task tools
        let required_tool = task.required_tools.first().map(|s| s.as_str()).unwrap_or("");
        if required_tool.is_empty() {
            return None;
        }

        // Ask cluster manager to find a node with matching agent
        cluster.route_to_node(required_tool).map(|n| n.address.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registration(id: &str, agent_type: &str, tools: Vec<&str>) -> AgentRegistration {
        AgentRegistration {
            agent_id: id.to_string(),
            agent_type: agent_type.to_string(),
            capabilities: vec![],
            tool_namespaces: tools.into_iter().map(|s| s.to_string()).collect(),
            status: "idle".to_string(),
            registered_at: 0,
        }
    }

    #[tokio::test]
    async fn test_register_and_list() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-1", "system", vec!["fs", "process"]))
            .await;

        let agents = router.list_agents().await;
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "agent-1");
    }

    #[tokio::test]
    async fn test_route_task() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("sys-1", "system", vec!["fs", "process"]))
            .await;
        router
            .register_agent(make_registration("net-1", "network", vec!["net", "firewall"]))
            .await;

        let task = Task {
            id: "task-1".into(),
            goal_id: "goal-1".into(),
            description: "Read file".into(),
            assigned_agent: String::new(),
            status: "pending".into(),
            intelligence_level: "operational".into(),
            required_tools: vec!["fs".into()],
            depends_on: vec![],
            input_json: vec![],
            output_json: vec![],
            created_at: 0,
            started_at: 0,
            completed_at: 0,
            error: String::new(),
        };

        let agent = router.route_task(&task);
        assert_eq!(agent, Some("sys-1".to_string()));
    }

    fn make_task(tools: Vec<&str>) -> Task {
        Task {
            id: "task-1".into(),
            goal_id: "goal-1".into(),
            description: "Test task".into(),
            assigned_agent: String::new(),
            status: "pending".into(),
            intelligence_level: "operational".into(),
            required_tools: tools.into_iter().map(|s| s.to_string()).collect(),
            depends_on: vec![],
            input_json: vec![],
            output_json: vec![],
            created_at: 0,
            started_at: 0,
            completed_at: 0,
            error: String::new(),
        }
    }

    #[tokio::test]
    async fn test_unregister_agent() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-1", "system", vec!["fs"]))
            .await;

        assert_eq!(router.active_agent_count(), 1);

        router.unregister_agent("agent-1").await;
        assert_eq!(router.active_agent_count(), 0);

        let agents = router.list_agents().await;
        assert!(agents.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_nonexistent() {
        let mut router = AgentRouter::new();
        // Should not panic
        router.unregister_agent("nonexistent").await;
    }

    #[test]
    fn test_update_heartbeat() {
        let mut router = AgentRouter::new();
        // Must register first via sync method -- we'll test the heartbeat directly
        let reg = make_registration("agent-1", "system", vec!["fs"]);
        router.agents.insert(
            "agent-1".to_string(),
            TrackedAgent {
                registration: reg,
                last_heartbeat: Instant::now(),
                status: "idle".to_string(),
                current_task: None,
                tasks_completed: 0,
                tasks_failed: 0,
            },
        );

        router.update_heartbeat("agent-1", "busy");
        let agent = router.agents.get("agent-1").unwrap();
        assert_eq!(agent.status, "busy");
    }

    #[test]
    fn test_update_heartbeat_nonexistent() {
        let mut router = AgentRouter::new();
        // Should not panic
        router.update_heartbeat("nonexistent", "idle");
    }

    #[tokio::test]
    async fn test_route_task_no_tools_required() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-1", "system", vec!["fs"]))
            .await;

        // Task with no required_tools should NOT match any agent
        // (falls through to AI inference which knows the actual tool names)
        let task = make_task(vec![]);
        let agent = router.route_task(&task);
        assert_eq!(agent, None);
    }

    #[tokio::test]
    async fn test_route_task_no_matching_agent() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-1", "system", vec!["fs"]))
            .await;

        // Task requiring "net" but agent only has "fs"
        let task = make_task(vec!["net"]);
        let agent = router.route_task(&task);
        // Falls back to the second search which checks all healthy agents
        // This agent doesn't have "net" in tool_namespaces either, so no match
        assert!(agent.is_none());
    }

    #[tokio::test]
    async fn test_route_prefers_idle_agent() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-1", "system", vec!["fs"]))
            .await;
        router
            .register_agent(make_registration("agent-2", "system", vec!["fs"]))
            .await;

        // Make agent-1 busy
        router.assign_task("agent-1", "task-x");

        let task = make_task(vec!["fs"]);
        let agent = router.route_task(&task);
        assert_eq!(agent, Some("agent-2".to_string()));
    }

    #[test]
    fn test_assign_task() {
        let mut router = AgentRouter::new();
        let reg = make_registration("agent-1", "system", vec!["fs"]);
        router.agents.insert(
            "agent-1".to_string(),
            TrackedAgent {
                registration: reg,
                last_heartbeat: Instant::now(),
                status: "idle".to_string(),
                current_task: None,
                tasks_completed: 0,
                tasks_failed: 0,
            },
        );

        router.assign_task("agent-1", "task-1");
        let agent = router.agents.get("agent-1").unwrap();
        assert_eq!(agent.status, "busy");
        assert_eq!(agent.current_task, Some("task-1".to_string()));
    }

    #[test]
    fn test_task_completed_success() {
        let mut router = AgentRouter::new();
        let reg = make_registration("agent-1", "system", vec!["fs"]);
        router.agents.insert(
            "agent-1".to_string(),
            TrackedAgent {
                registration: reg,
                last_heartbeat: Instant::now(),
                status: "busy".to_string(),
                current_task: Some("task-1".to_string()),
                tasks_completed: 0,
                tasks_failed: 0,
            },
        );

        router.task_completed("agent-1", true);
        let agent = router.agents.get("agent-1").unwrap();
        assert_eq!(agent.status, "idle");
        assert!(agent.current_task.is_none());
        assert_eq!(agent.tasks_completed, 1);
        assert_eq!(agent.tasks_failed, 0);
    }

    #[test]
    fn test_task_completed_failure() {
        let mut router = AgentRouter::new();
        let reg = make_registration("agent-1", "system", vec!["fs"]);
        router.agents.insert(
            "agent-1".to_string(),
            TrackedAgent {
                registration: reg,
                last_heartbeat: Instant::now(),
                status: "busy".to_string(),
                current_task: Some("task-1".to_string()),
                tasks_completed: 0,
                tasks_failed: 0,
            },
        );

        router.task_completed("agent-1", false);
        let agent = router.agents.get("agent-1").unwrap();
        assert_eq!(agent.status, "idle");
        assert_eq!(agent.tasks_completed, 0);
        assert_eq!(agent.tasks_failed, 1);
    }

    #[tokio::test]
    async fn test_active_agent_count() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-1", "system", vec!["fs"]))
            .await;
        router
            .register_agent(make_registration("agent-2", "system", vec!["net"]))
            .await;

        assert_eq!(router.active_agent_count(), 2);
    }

    #[test]
    fn test_dead_agents_empty() {
        let router = AgentRouter::new();
        assert!(router.dead_agents().is_empty());
    }

    #[tokio::test]
    async fn test_list_agents_reflects_status() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-1", "system", vec!["fs"]))
            .await;

        router.assign_task("agent-1", "task-1");

        let agents = router.list_agents().await;
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].status, "busy");
    }

    #[tokio::test]
    async fn test_route_prefers_experienced_agent() {
        let mut router = AgentRouter::new();
        router
            .register_agent(make_registration("agent-new", "system", vec!["fs"]))
            .await;
        router
            .register_agent(make_registration("agent-exp", "system", vec!["fs"]))
            .await;

        // Give agent-exp some completed tasks
        if let Some(agent) = router.agents.get_mut("agent-exp") {
            agent.tasks_completed = 10;
        }

        let task = make_task(vec!["fs"]);
        let selected = router.route_task(&task);
        // Should prefer the more experienced agent
        assert_eq!(selected, Some("agent-exp".to_string()));
    }
}
