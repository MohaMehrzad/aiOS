//! Task Planner — decomposes goals into executable tasks
//!
//! Uses local AI models to break down goals into a DAG of tasks,
//! determines intelligence levels, and identifies required tools.

use anyhow::Result;
use std::collections::HashMap;
use uuid::Uuid;

use crate::proto::common::Task;

/// Intelligence levels for task routing
#[derive(Debug, Clone, PartialEq)]
pub enum IntelligenceLevel {
    /// Simple heuristic — no AI needed
    Reactive,
    /// Small local model (TinyLlama 1.1B)
    Operational,
    /// Medium local model (Mistral 7B)
    Tactical,
    /// External API (Claude/OpenAI)
    Strategic,
}

impl IntelligenceLevel {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Reactive => "reactive",
            Self::Operational => "operational",
            Self::Tactical => "tactical",
            Self::Strategic => "strategic",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "reactive" => Self::Reactive,
            "operational" => Self::Operational,
            "tactical" => Self::Tactical,
            "strategic" => Self::Strategic,
            _ => Self::Operational,
        }
    }
}

/// Task planner state
pub struct TaskPlanner {
    pending_tasks: HashMap<String, Task>,
    _task_dependencies: HashMap<String, Vec<String>>,
}

impl TaskPlanner {
    pub fn new() -> Self {
        Self {
            pending_tasks: HashMap::new(),
            _task_dependencies: HashMap::new(),
        }
    }

    /// Decompose a goal into tasks
    ///
    /// For simple goals, uses heuristic decomposition.
    /// For complex goals, calls the AI runtime for decomposition.
    pub async fn decompose_goal(
        &mut self,
        goal_id: &str,
        description: &str,
    ) -> Result<Vec<Task>> {
        let level = self.classify_complexity(description);

        let tasks = match level {
            IntelligenceLevel::Reactive => {
                self.heuristic_decompose(goal_id, description).await?
            }
            _ => {
                // Non-reactive tasks produce a single task; the AI runtime
                // can further decompose complex tasks at execution time.
                self.single_task_decompose(goal_id, description, &level)
                    .await?
            }
        };

        // Register tasks
        for task in &tasks {
            self.pending_tasks.insert(task.id.clone(), task.clone());
        }

        Ok(tasks)
    }

    /// Classify the intelligence level needed for a goal
    pub fn classify_complexity(&self, description: &str) -> IntelligenceLevel {
        let desc_lower = description.to_lowercase();

        // Reactive: simple status checks, health checks
        if desc_lower.contains("status")
            || desc_lower.contains("health")
            || desc_lower.contains("uptime")
            || desc_lower.contains("ping")
        {
            return IntelligenceLevel::Reactive;
        }

        // Strategic: complex reasoning, planning, security analysis
        if desc_lower.contains("analyze")
            || desc_lower.contains("plan")
            || desc_lower.contains("design")
            || desc_lower.contains("security audit")
            || desc_lower.contains("architecture")
        {
            return IntelligenceLevel::Strategic;
        }

        // Operational: simple file operations, basic monitoring
        if desc_lower.contains("read file")
            || desc_lower.contains("list")
            || desc_lower.contains("check disk")
            || desc_lower.contains("log")
        {
            return IntelligenceLevel::Operational;
        }

        // Default to tactical for everything else
        IntelligenceLevel::Tactical
    }

    /// Simple heuristic decomposition for reactive tasks
    async fn heuristic_decompose(
        &self,
        goal_id: &str,
        description: &str,
    ) -> Result<Vec<Task>> {
        let now = chrono::Utc::now().timestamp();
        let task = Task {
            id: Uuid::new_v4().to_string(),
            goal_id: goal_id.to_string(),
            description: description.to_string(),
            assigned_agent: String::new(),
            status: "pending".to_string(),
            intelligence_level: "reactive".to_string(),
            required_tools: vec![],
            depends_on: vec![],
            input_json: vec![],
            output_json: vec![],
            created_at: now,
            started_at: 0,
            completed_at: 0,
            error: String::new(),
        };
        Ok(vec![task])
    }

    /// Create a single task for the goal
    async fn single_task_decompose(
        &self,
        goal_id: &str,
        description: &str,
        level: &IntelligenceLevel,
    ) -> Result<Vec<Task>> {
        let now = chrono::Utc::now().timestamp();
        let tools = self.infer_required_tools(description);

        let task = Task {
            id: Uuid::new_v4().to_string(),
            goal_id: goal_id.to_string(),
            description: description.to_string(),
            assigned_agent: String::new(),
            status: "pending".to_string(),
            intelligence_level: level.as_str().to_string(),
            required_tools: tools,
            depends_on: vec![],
            input_json: vec![],
            output_json: vec![],
            created_at: now,
            started_at: 0,
            completed_at: 0,
            error: String::new(),
        };
        Ok(vec![task])
    }

    /// Infer which tools a task might need based on description keywords
    fn infer_required_tools(&self, description: &str) -> Vec<String> {
        let desc_lower = description.to_lowercase();
        let mut tools = vec![];

        if desc_lower.contains("file")
            || desc_lower.contains("read")
            || desc_lower.contains("write")
            || desc_lower.contains("directory")
        {
            tools.push("fs".to_string());
        }
        if desc_lower.contains("process")
            || desc_lower.contains("kill")
            || desc_lower.contains("spawn")
        {
            tools.push("process".to_string());
        }
        if desc_lower.contains("service")
            || desc_lower.contains("restart")
            || desc_lower.contains("start")
            || desc_lower.contains("stop")
        {
            tools.push("service".to_string());
        }
        if desc_lower.contains("network")
            || desc_lower.contains("firewall")
            || desc_lower.contains("dns")
        {
            tools.push("net".to_string());
        }
        if desc_lower.contains("install")
            || desc_lower.contains("package")
            || desc_lower.contains("update")
        {
            tools.push("pkg".to_string());
        }
        if desc_lower.contains("security")
            || desc_lower.contains("permission")
            || desc_lower.contains("audit")
        {
            tools.push("sec".to_string());
        }

        tools
    }

    /// Get count of pending tasks
    pub fn pending_task_count(&self) -> usize {
        self.pending_tasks
            .values()
            .filter(|t| t.status == "pending")
            .count()
    }

    /// Mark a task as completed
    pub fn complete_task(&mut self, task_id: &str, output: Vec<u8>) {
        if let Some(task) = self.pending_tasks.get_mut(task_id) {
            task.status = "completed".to_string();
            task.output_json = output;
            task.completed_at = chrono::Utc::now().timestamp();
        }
    }

    /// Mark a task as failed
    pub fn fail_task(&mut self, task_id: &str, error: &str) {
        if let Some(task) = self.pending_tasks.get_mut(task_id) {
            task.status = "failed".to_string();
            task.error = error.to_string();
            task.completed_at = chrono::Utc::now().timestamp();
        }
    }

    /// Get next unblocked pending task
    pub fn next_task(&self) -> Option<&Task> {
        self.pending_tasks
            .values()
            .filter(|t| t.status == "pending")
            .find(|t| {
                // Check all dependencies are completed
                t.depends_on.iter().all(|dep_id| {
                    self.pending_tasks
                        .get(dep_id)
                        .map_or(true, |dep| dep.status == "completed")
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_reactive() {
        let planner = TaskPlanner::new();
        assert_eq!(
            planner.classify_complexity("Check system health status"),
            IntelligenceLevel::Reactive
        );
    }

    #[test]
    fn test_classify_operational() {
        let planner = TaskPlanner::new();
        assert_eq!(
            planner.classify_complexity("Read file /etc/hostname"),
            IntelligenceLevel::Operational
        );
    }

    #[test]
    fn test_classify_strategic() {
        let planner = TaskPlanner::new();
        assert_eq!(
            planner.classify_complexity("Analyze security audit logs"),
            IntelligenceLevel::Strategic
        );
    }

    #[tokio::test]
    async fn test_decompose_goal() {
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Check system health status")
            .await
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].intelligence_level, "reactive");
    }

    #[test]
    fn test_infer_tools() {
        let planner = TaskPlanner::new();
        let tools = planner.infer_required_tools("Read the contents of /etc/hostname file");
        assert!(tools.contains(&"fs".to_string()));
    }

    #[test]
    fn test_classify_tactical_default() {
        let planner = TaskPlanner::new();
        // Something that doesn't match any specific category
        assert_eq!(
            planner.classify_complexity("Compile the codebase"),
            IntelligenceLevel::Tactical
        );
    }

    #[test]
    fn test_classify_reactive_variants() {
        let planner = TaskPlanner::new();
        assert_eq!(
            planner.classify_complexity("Ping the server"),
            IntelligenceLevel::Reactive
        );
        assert_eq!(
            planner.classify_complexity("Check uptime of services"),
            IntelligenceLevel::Reactive
        );
    }

    #[test]
    fn test_classify_operational_variants() {
        let planner = TaskPlanner::new();
        assert_eq!(
            planner.classify_complexity("List all running processes"),
            IntelligenceLevel::Operational
        );
        assert_eq!(
            planner.classify_complexity("Check disk usage"),
            IntelligenceLevel::Operational
        );
        assert_eq!(
            planner.classify_complexity("Show recent log entries"),
            IntelligenceLevel::Operational
        );
    }

    #[test]
    fn test_classify_strategic_variants() {
        let planner = TaskPlanner::new();
        assert_eq!(
            planner.classify_complexity("Plan the deployment strategy"),
            IntelligenceLevel::Strategic
        );
        assert_eq!(
            planner.classify_complexity("Design a new architecture"),
            IntelligenceLevel::Strategic
        );
    }

    #[test]
    fn test_intelligence_level_roundtrip() {
        let levels = vec![
            IntelligenceLevel::Reactive,
            IntelligenceLevel::Operational,
            IntelligenceLevel::Tactical,
            IntelligenceLevel::Strategic,
        ];
        for level in levels {
            let s = level.as_str();
            let recovered = IntelligenceLevel::from_str(s);
            assert_eq!(level, recovered);
        }
    }

    #[test]
    fn test_intelligence_level_from_unknown() {
        assert_eq!(
            IntelligenceLevel::from_str("unknown_level"),
            IntelligenceLevel::Operational
        );
    }

    #[test]
    fn test_infer_tools_process() {
        let planner = TaskPlanner::new();
        let tools = planner.infer_required_tools("Kill the process that uses too much CPU");
        assert!(tools.contains(&"process".to_string()));
    }

    #[test]
    fn test_infer_tools_service() {
        let planner = TaskPlanner::new();
        let tools = planner.infer_required_tools("Restart the nginx service");
        assert!(tools.contains(&"service".to_string()));
    }

    #[test]
    fn test_infer_tools_network() {
        let planner = TaskPlanner::new();
        let tools = planner.infer_required_tools("Check DNS resolution for example.com");
        assert!(tools.contains(&"net".to_string()));
    }

    #[test]
    fn test_infer_tools_package() {
        let planner = TaskPlanner::new();
        let tools = planner.infer_required_tools("Install the curl package");
        assert!(tools.contains(&"pkg".to_string()));
    }

    #[test]
    fn test_infer_tools_security() {
        let planner = TaskPlanner::new();
        let tools = planner.infer_required_tools("Audit file permission settings");
        assert!(tools.contains(&"sec".to_string()));
    }

    #[test]
    fn test_infer_tools_multiple() {
        let planner = TaskPlanner::new();
        let tools =
            planner.infer_required_tools("Read file and check security permission settings");
        assert!(tools.contains(&"fs".to_string()));
        assert!(tools.contains(&"sec".to_string()));
    }

    #[test]
    fn test_infer_tools_none() {
        let planner = TaskPlanner::new();
        let tools = planner.infer_required_tools("Hello world");
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_decompose_strategic_goal() {
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Analyze security audit findings")
            .await
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].intelligence_level, "strategic");
        assert_eq!(tasks[0].goal_id, "goal-1");
        assert!(tasks[0].required_tools.contains(&"sec".to_string()));
    }

    #[tokio::test]
    async fn test_decompose_registers_pending_tasks() {
        let mut planner = TaskPlanner::new();
        assert_eq!(planner.pending_task_count(), 0);

        planner
            .decompose_goal("goal-1", "Check system health status")
            .await
            .unwrap();
        assert_eq!(planner.pending_task_count(), 1);

        planner
            .decompose_goal("goal-2", "Read file /etc/hosts")
            .await
            .unwrap();
        assert_eq!(planner.pending_task_count(), 2);
    }

    #[tokio::test]
    async fn test_complete_task() {
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Check status")
            .await
            .unwrap();
        let task_id = tasks[0].id.clone();

        assert_eq!(planner.pending_task_count(), 1);

        planner.complete_task(&task_id, b"result".to_vec());
        assert_eq!(planner.pending_task_count(), 0);

        let task = planner.pending_tasks.get(&task_id).unwrap();
        assert_eq!(task.status, "completed");
        assert_eq!(task.output_json, b"result".to_vec());
        assert!(task.completed_at > 0);
    }

    #[tokio::test]
    async fn test_fail_task() {
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Check status")
            .await
            .unwrap();
        let task_id = tasks[0].id.clone();

        planner.fail_task(&task_id, "timeout");
        let task = planner.pending_tasks.get(&task_id).unwrap();
        assert_eq!(task.status, "failed");
        assert_eq!(task.error, "timeout");
        assert!(task.completed_at > 0);
    }

    #[tokio::test]
    async fn test_next_task() {
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Check health")
            .await
            .unwrap();
        let task_id = tasks[0].id.clone();

        // Should return the pending task
        let next = planner.next_task();
        assert!(next.is_some());
        assert_eq!(next.unwrap().id, task_id);

        // After completing, no more pending
        planner.complete_task(&task_id, vec![]);
        let next = planner.next_task();
        assert!(next.is_none());
    }

    #[test]
    fn test_complete_nonexistent_task() {
        let mut planner = TaskPlanner::new();
        // Should not panic
        planner.complete_task("nonexistent", vec![]);
        planner.fail_task("nonexistent", "error");
    }

    #[tokio::test]
    async fn test_decompose_assigns_correct_timestamps() {
        let before = chrono::Utc::now().timestamp();
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Check system health")
            .await
            .unwrap();
        let after = chrono::Utc::now().timestamp();

        assert!(tasks[0].created_at >= before);
        assert!(tasks[0].created_at <= after);
        assert_eq!(tasks[0].started_at, 0);
        assert_eq!(tasks[0].completed_at, 0);
    }
}
