//! Result Aggregator — collects and processes task results
//!
//! Determines when a goal is complete by checking if all tasks
//! have finished, and aggregates results into a goal-level summary.

use std::collections::HashMap;
use tracing::info;

use crate::proto::common::TaskResult;

/// Stores task results and determines goal completion
pub struct ResultAggregator {
    results: HashMap<String, Vec<TaskResult>>,
}

impl ResultAggregator {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
        }
    }

    /// Record a task result
    pub fn record_result(&mut self, goal_id: &str, result: TaskResult) {
        info!(
            "Task {} completed (success: {}, tokens: {}, model: {})",
            result.task_id, result.success, result.tokens_used, result.model_used
        );

        self.results
            .entry(goal_id.to_string())
            .or_default()
            .push(result);
    }

    /// Check if all tasks for a goal have completed
    pub fn is_goal_complete(&self, goal_id: &str, expected_tasks: usize) -> bool {
        self.results
            .get(goal_id)
            .map_or(false, |results| results.len() >= expected_tasks)
    }

    /// Check if any task in a goal has failed
    pub fn has_failures(&self, goal_id: &str) -> bool {
        self.results
            .get(goal_id)
            .map_or(false, |results| results.iter().any(|r| !r.success))
    }

    /// Get total tokens used for a goal
    pub fn total_tokens(&self, goal_id: &str) -> i32 {
        self.results
            .get(goal_id)
            .map_or(0, |results| results.iter().map(|r| r.tokens_used).sum())
    }

    /// Get total duration for a goal
    pub fn total_duration_ms(&self, goal_id: &str) -> i64 {
        self.results
            .get(goal_id)
            .map_or(0, |results| results.iter().map(|r| r.duration_ms).sum())
    }

    /// Get aggregated result summary for a goal
    pub fn get_goal_summary(&self, goal_id: &str) -> GoalSummary {
        let results = self.results.get(goal_id);

        match results {
            Some(results) => {
                let total = results.len();
                let succeeded = results.iter().filter(|r| r.success).count();
                let failed = total - succeeded;
                let total_tokens: i32 = results.iter().map(|r| r.tokens_used).sum();
                let total_duration: i64 = results.iter().map(|r| r.duration_ms).sum();
                let models_used: Vec<String> = results
                    .iter()
                    .map(|r| r.model_used.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();

                GoalSummary {
                    total_tasks: total,
                    succeeded,
                    failed,
                    total_tokens,
                    total_duration_ms: total_duration,
                    models_used,
                    overall_success: failed == 0,
                }
            }
            None => GoalSummary::default(),
        }
    }

    /// Clear results for a completed goal (free memory)
    pub fn clear_goal(&mut self, goal_id: &str) {
        self.results.remove(goal_id);
    }
}

/// Summary of goal execution
#[derive(Debug, Default)]
pub struct GoalSummary {
    pub total_tasks: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub total_tokens: i32,
    pub total_duration_ms: i64,
    pub models_used: Vec<String>,
    pub overall_success: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_check() {
        let mut agg = ResultAggregator::new();

        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-1".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );

        assert!(agg.is_goal_complete("goal-1", 1));
        assert!(!agg.has_failures("goal-1"));
        assert_eq!(agg.total_tokens("goal-1"), 50);
    }

    #[test]
    fn test_goal_summary() {
        let mut agg = ResultAggregator::new();

        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-1".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-2".into(),
                success: false,
                output_json: vec![],
                error: "timeout".into(),
                duration_ms: 5000,
                tokens_used: 0,
                model_used: "mistral".into(),
            },
        );

        let summary = agg.get_goal_summary("goal-1");
        assert_eq!(summary.total_tasks, 2);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.failed, 1);
        assert!(!summary.overall_success);
    }

    #[test]
    fn test_is_goal_complete_not_enough_tasks() {
        let mut agg = ResultAggregator::new();
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-1".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );

        // Expected 2 tasks but only 1 recorded
        assert!(!agg.is_goal_complete("goal-1", 2));
        assert!(agg.is_goal_complete("goal-1", 1));
    }

    #[test]
    fn test_is_goal_complete_nonexistent() {
        let agg = ResultAggregator::new();
        assert!(!agg.is_goal_complete("nonexistent", 1));
    }

    #[test]
    fn test_has_failures_no_results() {
        let agg = ResultAggregator::new();
        assert!(!agg.has_failures("nonexistent"));
    }

    #[test]
    fn test_has_failures_all_success() {
        let mut agg = ResultAggregator::new();
        for i in 0..3 {
            agg.record_result(
                "goal-1",
                TaskResult {
                    task_id: format!("task-{i}"),
                    success: true,
                    output_json: vec![],
                    error: String::new(),
                    duration_ms: 100,
                    tokens_used: 50,
                    model_used: "tinyllama".into(),
                },
            );
        }
        assert!(!agg.has_failures("goal-1"));
    }

    #[test]
    fn test_total_tokens_multiple_results() {
        let mut agg = ResultAggregator::new();
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-1".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-2".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 200,
                tokens_used: 75,
                model_used: "mistral".into(),
            },
        );

        assert_eq!(agg.total_tokens("goal-1"), 125);
        assert_eq!(agg.total_tokens("nonexistent"), 0);
    }

    #[test]
    fn test_total_duration_ms() {
        let mut agg = ResultAggregator::new();
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-1".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-2".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 200,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );

        assert_eq!(agg.total_duration_ms("goal-1"), 300);
        assert_eq!(agg.total_duration_ms("nonexistent"), 0);
    }

    #[test]
    fn test_goal_summary_all_success() {
        let mut agg = ResultAggregator::new();
        for i in 0..3 {
            agg.record_result(
                "goal-1",
                TaskResult {
                    task_id: format!("task-{i}"),
                    success: true,
                    output_json: vec![],
                    error: String::new(),
                    duration_ms: 100,
                    tokens_used: 50,
                    model_used: "tinyllama".into(),
                },
            );
        }

        let summary = agg.get_goal_summary("goal-1");
        assert_eq!(summary.total_tasks, 3);
        assert_eq!(summary.succeeded, 3);
        assert_eq!(summary.failed, 0);
        assert!(summary.overall_success);
        assert_eq!(summary.total_tokens, 150);
        assert_eq!(summary.total_duration_ms, 300);
    }

    #[test]
    fn test_goal_summary_models_used_dedup() {
        let mut agg = ResultAggregator::new();
        // Two tasks with same model
        for i in 0..2 {
            agg.record_result(
                "goal-1",
                TaskResult {
                    task_id: format!("task-{i}"),
                    success: true,
                    output_json: vec![],
                    error: String::new(),
                    duration_ms: 100,
                    tokens_used: 50,
                    model_used: "tinyllama".into(),
                },
            );
        }
        // One task with different model
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-3".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "mistral".into(),
            },
        );

        let summary = agg.get_goal_summary("goal-1");
        assert_eq!(summary.models_used.len(), 2);
        assert!(summary.models_used.contains(&"tinyllama".to_string()));
        assert!(summary.models_used.contains(&"mistral".to_string()));
    }

    #[test]
    fn test_goal_summary_nonexistent() {
        let agg = ResultAggregator::new();
        let summary = agg.get_goal_summary("nonexistent");
        assert_eq!(summary.total_tasks, 0);
        assert_eq!(summary.succeeded, 0);
        assert_eq!(summary.failed, 0);
        assert!(!summary.overall_success);
    }

    #[test]
    fn test_clear_goal() {
        let mut agg = ResultAggregator::new();
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-1".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );

        assert!(agg.is_goal_complete("goal-1", 1));
        agg.clear_goal("goal-1");
        assert!(!agg.is_goal_complete("goal-1", 1));
    }

    #[test]
    fn test_clear_nonexistent_goal() {
        let mut agg = ResultAggregator::new();
        // Should not panic
        agg.clear_goal("nonexistent");
    }

    #[test]
    fn test_multiple_goals_isolation() {
        let mut agg = ResultAggregator::new();
        agg.record_result(
            "goal-1",
            TaskResult {
                task_id: "task-1".into(),
                success: true,
                output_json: vec![],
                error: String::new(),
                duration_ms: 100,
                tokens_used: 50,
                model_used: "tinyllama".into(),
            },
        );
        agg.record_result(
            "goal-2",
            TaskResult {
                task_id: "task-2".into(),
                success: false,
                output_json: vec![],
                error: "fail".into(),
                duration_ms: 200,
                tokens_used: 100,
                model_used: "mistral".into(),
            },
        );

        assert!(!agg.has_failures("goal-1"));
        assert!(agg.has_failures("goal-2"));
        assert_eq!(agg.total_tokens("goal-1"), 50);
        assert_eq!(agg.total_tokens("goal-2"), 100);
    }
}
