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

    /// Load persisted tasks into the planner (called on startup after
    /// GoalEngine restores from SQLite). This ensures tasks from previous
    /// sessions are picked up by the autonomy loop.
    pub fn load_persisted_tasks(&mut self, tasks: Vec<Task>) {
        let count = tasks.len();
        for task in tasks {
            self.pending_tasks.insert(task.id.clone(), task);
        }
        if count > 0 {
            tracing::info!("TaskPlanner loaded {count} persisted tasks");
        }
    }

    /// Decompose a goal into tasks
    ///
    /// For simple goals, uses heuristic decomposition.
    /// For Tactical/Strategic goals, uses AI-powered multi-step decomposition.
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
            IntelligenceLevel::Operational => {
                self.single_task_decompose(goal_id, description, &level)
                    .await?
            }
            IntelligenceLevel::Tactical | IntelligenceLevel::Strategic => {
                // AI-powered decomposition: break into multiple steps
                self.ai_decompose(goal_id, description, &level).await?
            }
        };

        // Register tasks
        for task in &tasks {
            self.pending_tasks.insert(task.id.clone(), task.clone());
        }

        Ok(tasks)
    }

    /// AI-powered decomposition for complex goals
    /// Generates a multi-step task plan based on goal analysis
    async fn ai_decompose(
        &mut self,
        goal_id: &str,
        description: &str,
        level: &IntelligenceLevel,
    ) -> Result<Vec<Task>> {
        let now = chrono::Utc::now().timestamp();

        // Analyze the goal to determine sub-tasks
        let subtasks = self.analyze_goal_steps(description);
        let mut tasks = Vec::new();
        let mut prev_task_id: Option<String> = None;

        for (i, (subdesc, tools)) in subtasks.into_iter().enumerate() {
            let task_id = Uuid::new_v4().to_string();
            let depends_on = if let Some(ref prev) = prev_task_id {
                vec![prev.clone()]
            } else {
                vec![]
            };

            tasks.push(Task {
                id: task_id.clone(),
                goal_id: goal_id.to_string(),
                description: subdesc,
                assigned_agent: String::new(),
                status: "pending".to_string(),
                intelligence_level: if i == 0 {
                    // First task may be simpler (gather info)
                    "operational".to_string()
                } else {
                    level.as_str().to_string()
                },
                required_tools: tools,
                depends_on,
                input_json: vec![],
                output_json: vec![],
                created_at: now,
                started_at: 0,
                completed_at: 0,
                error: String::new(),
            });

            // Build dependency chain
            if let Some(prev) = &prev_task_id {
                self._task_dependencies
                    .entry(task_id.clone())
                    .or_default()
                    .push(prev.clone());
            }
            prev_task_id = Some(task_id);
        }

        // If analysis produced nothing, fall back to single task
        if tasks.is_empty() {
            return self.single_task_decompose(goal_id, description, level).await;
        }

        Ok(tasks)
    }

    /// Analyze goal description to determine steps
    /// Uses keyword heuristics to generate multi-step plans
    fn analyze_goal_steps(&self, description: &str) -> Vec<(String, Vec<String>)> {
        let desc_lower = description.to_lowercase();
        let mut steps = Vec::new();

        // Service management goals
        if desc_lower.contains("restart") || desc_lower.contains("deploy") {
            let service = extract_service_name(&desc_lower);
            steps.push((
                format!("Check current status of {service}"),
                vec!["service".to_string(), "monitor".to_string()],
            ));
            steps.push((
                format!("Stop {service} gracefully"),
                vec!["service".to_string()],
            ));
            steps.push((
                format!("Start {service} and verify"),
                vec!["service".to_string(), "monitor".to_string()],
            ));
            return steps;
        }

        // Security analysis goals
        if desc_lower.contains("security") || desc_lower.contains("audit") {
            steps.push((
                "Gather system security configuration".to_string(),
                vec!["sec".to_string(), "fs".to_string()],
            ));
            steps.push((
                "Analyze security posture and vulnerabilities".to_string(),
                vec!["sec".to_string()],
            ));
            steps.push((
                "Generate security report with recommendations".to_string(),
                vec!["fs".to_string()],
            ));
            return steps;
        }

        // Installation goals
        if desc_lower.contains("install") || desc_lower.contains("setup") {
            steps.push((
                format!("Check prerequisites for: {description}"),
                vec!["pkg".to_string(), "fs".to_string()],
            ));
            steps.push((
                format!("Install: {description}"),
                vec!["pkg".to_string()],
            ));
            steps.push((
                "Verify installation and configure".to_string(),
                vec!["service".to_string(), "fs".to_string()],
            ));
            return steps;
        }

        // Network troubleshooting
        if desc_lower.contains("network") || desc_lower.contains("connectivity") {
            steps.push((
                "Check network interfaces and routing".to_string(),
                vec!["net".to_string()],
            ));
            steps.push((
                "Test DNS resolution and connectivity".to_string(),
                vec!["net".to_string()],
            ));
            steps.push((
                "Diagnose and apply fixes".to_string(),
                vec!["net".to_string(), "firewall".to_string()],
            ));
            return steps;
        }

        // Default: no multi-step decomposition
        steps
    }

    /// Classify the intelligence level needed for a goal
    pub fn classify_complexity(&self, description: &str) -> IntelligenceLevel {
        let desc_lower = description.to_lowercase();

        // Reactive: simple status checks, health checks, direct tool calls
        if desc_lower.contains("status")
            || desc_lower.contains("health")
            || desc_lower.contains("uptime")
            || desc_lower.contains("ping")
        {
            return IntelligenceLevel::Reactive;
        }

        // Reactive: email sending (heuristic execution can handle this directly)
        if (desc_lower.contains("email") || desc_lower.contains("mail"))
            && (desc_lower.contains("send") || desc_lower.contains("@"))
        {
            return IntelligenceLevel::Reactive;
        }

        // Reactive: explicit tool call in description (e.g. "call monitor.cpu")
        if desc_lower.contains("call ") || desc_lower.contains("execute ") || desc_lower.contains("run ") {
            let tool_patterns = ["fs.", "process.", "service.", "net.", "monitor.", "email.", "pkg.", "sec."];
            if tool_patterns.iter().any(|p| desc_lower.contains(p)) {
                return IntelligenceLevel::Reactive;
            }
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

        // Helper: check if word appears as a standalone word (not part of another word)
        let has_word = |text: &str, word: &str| -> bool {
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|w| w == word)
        };

        if desc_lower.contains("file")
            || has_word(&desc_lower, "read")
            || has_word(&desc_lower, "write")
            || desc_lower.contains("directory")
            || desc_lower.contains("disk")
        {
            tools.push("fs".to_string());
        }
        if desc_lower.contains("process")
            || has_word(&desc_lower, "kill")
            || has_word(&desc_lower, "spawn")
        {
            tools.push("process".to_string());
        }
        if has_word(&desc_lower, "service")
            || has_word(&desc_lower, "restart")
            || has_word(&desc_lower, "systemctl")
        {
            tools.push("service".to_string());
        }
        if desc_lower.contains("network")
            || desc_lower.contains("firewall")
            || has_word(&desc_lower, "dns")
            || has_word(&desc_lower, "ping")
        {
            tools.push("net".to_string());
        }
        if has_word(&desc_lower, "install")
            || has_word(&desc_lower, "package")
            || has_word(&desc_lower, "apt")
        {
            tools.push("pkg".to_string());
        }
        if desc_lower.contains("security")
            || desc_lower.contains("permission")
            || has_word(&desc_lower, "audit")
            || desc_lower.contains("vulnerab")
        {
            tools.push("sec".to_string());
        }
        if desc_lower.contains("plugin")
            || desc_lower.contains("script")
        {
            tools.push("plugin".to_string());
        }
        if desc_lower.contains("email")
            || desc_lower.contains("smtp")
            || desc_lower.contains("mail")
            || desc_lower.contains("newsletter")
        {
            tools.push("email".to_string());
        }
        if desc_lower.contains("monitor")
            || has_word(&desc_lower, "cpu")
            || has_word(&desc_lower, "memory")
            || desc_lower.contains("metric")
        {
            tools.push("monitor".to_string());
        }
        if desc_lower.contains("container")
            || desc_lower.contains("podman")
            || desc_lower.contains("docker")
        {
            tools.push("container".to_string());
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

    /// Mark a task as in-progress
    pub fn mark_in_progress(&mut self, task_id: &str) {
        if let Some(task) = self.pending_tasks.get_mut(task_id) {
            task.status = "in_progress".to_string();
            task.started_at = chrono::Utc::now().timestamp();
        }
    }

    /// Mark a task as awaiting user input
    pub fn mark_awaiting_input(&mut self, task_id: &str) {
        if let Some(task) = self.pending_tasks.get_mut(task_id) {
            task.status = "awaiting_input".to_string();
        }
    }

    /// Resume a task that was awaiting input (re-queue as pending)
    pub fn resume_task(&mut self, task_id: &str) {
        if let Some(task) = self.pending_tasks.get_mut(task_id) {
            task.status = "pending".to_string();
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

    /// Get a single task by ID
    pub fn get_task(&self, task_id: &str) -> Option<&Task> {
        self.pending_tasks.get(task_id)
    }

    /// Get all tasks for a goal
    pub fn get_tasks_for_goal(&self, goal_id: &str) -> Vec<&Task> {
        self.pending_tasks
            .values()
            .filter(|t| t.goal_id == goal_id)
            .collect()
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

/// Extract a service name from a goal description
fn extract_service_name(desc: &str) -> String {
    let known_services = [
        "nginx", "apache", "postgres", "mysql", "redis", "docker",
        "ssh", "systemd", "cron", "mongodb", "elasticsearch",
    ];
    for svc in &known_services {
        if desc.contains(svc) {
            return svc.to_string();
        }
    }
    "the service".to_string()
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
        // Security goals are decomposed into multiple steps
        assert!(tasks.len() >= 2);
        assert_eq!(tasks[0].goal_id, "goal-1");
        // First task should be operational (gather info)
        assert_eq!(tasks[0].intelligence_level, "operational");
    }

    #[tokio::test]
    async fn test_ai_decompose_service_restart() {
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Restart the nginx service")
            .await
            .unwrap();
        // Service restart goals produce 3 steps: check, stop, start
        assert_eq!(tasks.len(), 3);
        assert!(tasks[0].description.contains("status"));
        // Second task depends on first
        assert!(!tasks[1].depends_on.is_empty());
    }

    #[tokio::test]
    async fn test_ai_decompose_install() {
        let mut planner = TaskPlanner::new();
        let tasks = planner
            .decompose_goal("goal-1", "Install and setup redis")
            .await
            .unwrap();
        assert_eq!(tasks.len(), 3);
        assert!(tasks[0].required_tools.contains(&"pkg".to_string()));
    }

    #[test]
    fn test_extract_service_name() {
        assert_eq!(extract_service_name("restart nginx gracefully"), "nginx");
        assert_eq!(extract_service_name("deploy postgres"), "postgres");
        assert_eq!(extract_service_name("restart the app"), "the service");
    }

    #[test]
    fn test_get_tasks_for_goal() {
        let mut planner = TaskPlanner::new();
        // Manually insert tasks
        let task = Task {
            id: "t1".into(),
            goal_id: "g1".into(),
            description: "test".into(),
            assigned_agent: String::new(),
            status: "pending".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec![],
            depends_on: vec![],
            input_json: vec![],
            output_json: vec![],
            created_at: 0,
            started_at: 0,
            completed_at: 0,
            error: String::new(),
        };
        planner.pending_tasks.insert("t1".into(), task);
        assert_eq!(planner.get_tasks_for_goal("g1").len(), 1);
        assert_eq!(planner.get_tasks_for_goal("g2").len(), 0);
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
