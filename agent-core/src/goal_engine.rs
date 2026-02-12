//! Goal Engine — manages the lifecycle of goals
//!
//! Goals flow through: Pending → Planning → InProgress → Completed/Failed

use anyhow::Result;
use std::collections::HashMap;
use uuid::Uuid;

use crate::proto::common::{Goal, Task};

/// Manages goals and their lifecycle
pub struct GoalEngine {
    goals: HashMap<String, Goal>,
    goal_tasks: HashMap<String, Vec<Task>>,
}

impl GoalEngine {
    pub fn new() -> Self {
        Self {
            goals: HashMap::new(),
            goal_tasks: HashMap::new(),
        }
    }

    /// Submit a new goal
    pub async fn submit_goal(
        &mut self,
        description: String,
        priority: i32,
        source: String,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();

        let goal = Goal {
            id: id.clone(),
            description,
            priority,
            source,
            status: "pending".to_string(),
            created_at: now,
            updated_at: now,
            tags: vec![],
            metadata_json: vec![],
        };

        self.goals.insert(id.clone(), goal);
        self.goal_tasks.insert(id.clone(), vec![]);

        tracing::info!("Goal submitted: {id}");
        Ok(id)
    }

    /// Get a goal with its tasks
    pub async fn get_goal_with_tasks(&self, goal_id: &str) -> Result<(Goal, Vec<Task>)> {
        let goal = self
            .goals
            .get(goal_id)
            .ok_or_else(|| anyhow::anyhow!("Goal not found: {goal_id}"))?
            .clone();

        let tasks = self.goal_tasks.get(goal_id).cloned().unwrap_or_default();

        Ok((goal, tasks))
    }

    /// Calculate progress percentage for a goal
    pub async fn calculate_progress(&self, goal_id: &str) -> f64 {
        let tasks = match self.goal_tasks.get(goal_id) {
            Some(t) => t,
            None => return 0.0,
        };

        if tasks.is_empty() {
            return 0.0;
        }

        let completed = tasks
            .iter()
            .filter(|t| t.status == "completed")
            .count() as f64;
        let total = tasks.len() as f64;

        (completed / total) * 100.0
    }

    /// Cancel a goal
    pub async fn cancel_goal(&mut self, goal_id: &str) -> Result<()> {
        let goal = self
            .goals
            .get_mut(goal_id)
            .ok_or_else(|| anyhow::anyhow!("Goal not found: {goal_id}"))?;

        goal.status = "cancelled".to_string();
        goal.updated_at = chrono::Utc::now().timestamp();

        // Cancel all associated tasks
        if let Some(tasks) = self.goal_tasks.get_mut(goal_id) {
            for task in tasks.iter_mut() {
                if task.status != "completed" {
                    task.status = "cancelled".to_string();
                }
            }
        }

        tracing::info!("Goal cancelled: {goal_id}");
        Ok(())
    }

    /// List goals with filtering
    pub async fn list_goals(
        &self,
        status_filter: &str,
        limit: i32,
        offset: i32,
    ) -> (Vec<Goal>, i32) {
        let mut goals: Vec<&Goal> = if status_filter.is_empty() {
            self.goals.values().collect()
        } else {
            self.goals
                .values()
                .filter(|g| g.status == status_filter)
                .collect()
        };

        // Sort by priority (lower = higher priority) then by creation time
        goals.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(b.created_at.cmp(&a.created_at))
        });

        let total = goals.len() as i32;
        let offset = offset as usize;
        let limit = if limit <= 0 { 50 } else { limit as usize };

        let result = goals
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();

        (result, total)
    }

    /// Get count of active (non-terminal) goals
    pub fn active_goal_count(&self) -> usize {
        self.goals
            .values()
            .filter(|g| g.status != "completed" && g.status != "failed" && g.status != "cancelled")
            .count()
    }

    /// Add tasks to a goal
    pub fn add_tasks(&mut self, goal_id: &str, tasks: Vec<Task>) {
        if let Some(existing) = self.goal_tasks.get_mut(goal_id) {
            existing.extend(tasks);
        }
    }

    /// Mark a task within a goal as completed
    pub fn complete_task(&mut self, goal_id: &str, task_id: &str) {
        if let Some(tasks) = self.goal_tasks.get_mut(goal_id) {
            for task in tasks.iter_mut() {
                if task.id == task_id {
                    task.status = "completed".to_string();
                    break;
                }
            }
        }
    }

    /// Update goal status
    pub fn update_status(&mut self, goal_id: &str, status: &str) {
        if let Some(goal) = self.goals.get_mut(goal_id) {
            goal.status = status.to_string();
            goal.updated_at = chrono::Utc::now().timestamp();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_submit_goal() {
        let mut engine = GoalEngine::new();
        let id = engine
            .submit_goal("Test goal".into(), 2, "test".into())
            .await
            .unwrap();

        assert!(!id.is_empty());
        assert_eq!(engine.active_goal_count(), 1);
    }

    #[tokio::test]
    async fn test_cancel_goal() {
        let mut engine = GoalEngine::new();
        let id = engine
            .submit_goal("Test goal".into(), 2, "test".into())
            .await
            .unwrap();

        engine.cancel_goal(&id).await.unwrap();
        assert_eq!(engine.active_goal_count(), 0);
    }

    #[tokio::test]
    async fn test_list_goals() {
        let mut engine = GoalEngine::new();
        engine
            .submit_goal("Goal 1".into(), 2, "test".into())
            .await
            .unwrap();
        engine
            .submit_goal("Goal 2".into(), 1, "test".into())
            .await
            .unwrap();

        let (goals, total) = engine.list_goals("", 50, 0).await;
        assert_eq!(total, 2);
        assert_eq!(goals.len(), 2);
        // Higher priority (lower number) first
        assert_eq!(goals[0].priority, 1);
    }

    #[tokio::test]
    async fn test_get_goal_with_tasks() {
        let mut engine = GoalEngine::new();
        let id = engine
            .submit_goal("Test goal".into(), 1, "user".into())
            .await
            .unwrap();

        let (goal, tasks) = engine.get_goal_with_tasks(&id).await.unwrap();
        assert_eq!(goal.description, "Test goal");
        assert_eq!(goal.priority, 1);
        assert_eq!(goal.source, "user");
        assert_eq!(goal.status, "pending");
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn test_get_goal_not_found() {
        let engine = GoalEngine::new();
        let result = engine.get_goal_with_tasks("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Goal not found"));
    }

    #[tokio::test]
    async fn test_calculate_progress_no_tasks() {
        let mut engine = GoalEngine::new();
        let id = engine
            .submit_goal("Test".into(), 1, "test".into())
            .await
            .unwrap();
        let progress = engine.calculate_progress(&id).await;
        assert_eq!(progress, 0.0);
    }

    #[tokio::test]
    async fn test_calculate_progress_with_tasks() {
        let mut engine = GoalEngine::new();
        let id = engine
            .submit_goal("Test".into(), 1, "test".into())
            .await
            .unwrap();

        let task1 = Task {
            id: "t1".into(),
            goal_id: id.clone(),
            description: "Task 1".into(),
            assigned_agent: String::new(),
            status: "completed".into(),
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
        let task2 = Task {
            id: "t2".into(),
            goal_id: id.clone(),
            description: "Task 2".into(),
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

        engine.add_tasks(&id, vec![task1, task2]);
        let progress = engine.calculate_progress(&id).await;
        assert_eq!(progress, 50.0);
    }

    #[tokio::test]
    async fn test_calculate_progress_all_completed() {
        let mut engine = GoalEngine::new();
        let id = engine
            .submit_goal("Test".into(), 1, "test".into())
            .await
            .unwrap();

        let tasks: Vec<Task> = (0..3)
            .map(|i| Task {
                id: format!("t{i}"),
                goal_id: id.clone(),
                description: format!("Task {i}"),
                assigned_agent: String::new(),
                status: "completed".into(),
                intelligence_level: "reactive".into(),
                required_tools: vec![],
                depends_on: vec![],
                input_json: vec![],
                output_json: vec![],
                created_at: 0,
                started_at: 0,
                completed_at: 0,
                error: String::new(),
            })
            .collect();

        engine.add_tasks(&id, tasks);
        let progress = engine.calculate_progress(&id).await;
        assert_eq!(progress, 100.0);
    }

    #[tokio::test]
    async fn test_cancel_goal_cancels_pending_tasks() {
        let mut engine = GoalEngine::new();
        let id = engine
            .submit_goal("Test".into(), 1, "test".into())
            .await
            .unwrap();

        let task_completed = Task {
            id: "t1".into(),
            goal_id: id.clone(),
            description: "Done".into(),
            assigned_agent: String::new(),
            status: "completed".into(),
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
        let task_pending = Task {
            id: "t2".into(),
            goal_id: id.clone(),
            description: "Pending".into(),
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

        engine.add_tasks(&id, vec![task_completed, task_pending]);
        engine.cancel_goal(&id).await.unwrap();

        let (goal, tasks) = engine.get_goal_with_tasks(&id).await.unwrap();
        assert_eq!(goal.status, "cancelled");
        // Completed task stays completed, pending task gets cancelled
        assert_eq!(tasks[0].status, "completed");
        assert_eq!(tasks[1].status, "cancelled");
    }

    #[tokio::test]
    async fn test_cancel_nonexistent_goal() {
        let mut engine = GoalEngine::new();
        let result = engine.cancel_goal("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_goals_with_status_filter() {
        let mut engine = GoalEngine::new();
        let id1 = engine
            .submit_goal("Goal 1".into(), 1, "test".into())
            .await
            .unwrap();
        engine
            .submit_goal("Goal 2".into(), 2, "test".into())
            .await
            .unwrap();

        engine.update_status(&id1, "completed");

        let (pending, total_pending) = engine.list_goals("pending", 50, 0).await;
        assert_eq!(total_pending, 1);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, "pending");

        let (completed, total_completed) = engine.list_goals("completed", 50, 0).await;
        assert_eq!(total_completed, 1);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].status, "completed");
    }

    #[tokio::test]
    async fn test_list_goals_pagination() {
        let mut engine = GoalEngine::new();
        for i in 0..5 {
            engine
                .submit_goal(format!("Goal {i}"), i, "test".into())
                .await
                .unwrap();
        }

        let (page1, total) = engine.list_goals("", 2, 0).await;
        assert_eq!(total, 5);
        assert_eq!(page1.len(), 2);

        let (page2, _) = engine.list_goals("", 2, 2).await;
        assert_eq!(page2.len(), 2);

        let (page3, _) = engine.list_goals("", 2, 4).await;
        assert_eq!(page3.len(), 1);
    }

    #[tokio::test]
    async fn test_list_goals_default_limit() {
        let mut engine = GoalEngine::new();
        engine
            .submit_goal("Goal 1".into(), 1, "test".into())
            .await
            .unwrap();

        // limit=0 should default to 50
        let (goals, _) = engine.list_goals("", 0, 0).await;
        assert_eq!(goals.len(), 1);
    }

    #[test]
    fn test_active_goal_count_excludes_terminal_states() {
        let mut engine = GoalEngine::new();
        // Manually insert goals in various states
        let states = vec!["pending", "in_progress", "completed", "failed", "cancelled"];
        for (i, status) in states.iter().enumerate() {
            let id = format!("g{i}");
            engine.goals.insert(
                id.clone(),
                Goal {
                    id: id.clone(),
                    description: format!("Goal {i}"),
                    priority: 1,
                    source: "test".into(),
                    status: status.to_string(),
                    created_at: 0,
                    updated_at: 0,
                    tags: vec![],
                    metadata_json: vec![],
                },
            );
        }
        // Only pending and in_progress are active
        assert_eq!(engine.active_goal_count(), 2);
    }

    #[test]
    fn test_update_status() {
        let mut engine = GoalEngine::new();
        let id = "g1".to_string();
        engine.goals.insert(
            id.clone(),
            Goal {
                id: id.clone(),
                description: "Test".into(),
                priority: 1,
                source: "test".into(),
                status: "pending".into(),
                created_at: 100,
                updated_at: 100,
                tags: vec![],
                metadata_json: vec![],
            },
        );

        engine.update_status(&id, "in_progress");
        let goal = engine.goals.get(&id).unwrap();
        assert_eq!(goal.status, "in_progress");
        assert!(goal.updated_at >= 100);
    }

    #[test]
    fn test_add_tasks_to_nonexistent_goal() {
        let mut engine = GoalEngine::new();
        // Should not panic -- silently does nothing
        engine.add_tasks("nonexistent", vec![]);
    }

    #[tokio::test]
    async fn test_calculate_progress_nonexistent_goal() {
        let engine = GoalEngine::new();
        let progress = engine.calculate_progress("nonexistent").await;
        assert_eq!(progress, 0.0);
    }
}
