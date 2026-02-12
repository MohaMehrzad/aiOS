//! Decision Logger — records all AI decisions with reasoning
//!
//! Every decision the orchestrator makes is logged with:
//! - What options were considered
//! - What was chosen
//! - Why (reasoning)
//! - The outcome (updated after execution)

use std::collections::VecDeque;
use tracing::info;
use uuid::Uuid;

/// A recorded decision
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub id: String,
    pub timestamp: i64,
    pub context: String,
    pub options: Vec<String>,
    pub chosen: String,
    pub reasoning: String,
    pub intelligence_level: String,
    pub model_used: String,
    pub outcome: Option<String>,
}

/// Logs and stores all orchestrator decisions
pub struct DecisionLogger {
    decisions: VecDeque<DecisionRecord>,
    max_entries: usize,
}

impl DecisionLogger {
    pub fn new() -> Self {
        Self {
            decisions: VecDeque::new(),
            max_entries: 10000,
        }
    }

    /// Log a new decision
    pub fn log_decision(
        &mut self,
        context: &str,
        options: &[String],
        chosen: &str,
        reasoning: &str,
        intelligence_level: &str,
        model_used: &str,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();

        info!(
            "Decision [{id}]: context={context}, chosen={chosen}, reason={reasoning}"
        );

        let record = DecisionRecord {
            id: id.clone(),
            timestamp: now,
            context: context.to_string(),
            options: options.to_vec(),
            chosen: chosen.to_string(),
            reasoning: reasoning.to_string(),
            intelligence_level: intelligence_level.to_string(),
            model_used: model_used.to_string(),
            outcome: None,
        };

        self.decisions.push_back(record);

        // Trim if over capacity
        while self.decisions.len() > self.max_entries {
            self.decisions.pop_front();
        }

        id
    }

    /// Update the outcome of a previous decision
    pub fn update_outcome(&mut self, decision_id: &str, outcome: &str) {
        if let Some(decision) = self
            .decisions
            .iter_mut()
            .rev()
            .find(|d| d.id == decision_id)
        {
            decision.outcome = Some(outcome.to_string());
        }
    }

    /// Get recent decisions
    pub fn recent(&self, count: usize) -> Vec<&DecisionRecord> {
        self.decisions.iter().rev().take(count).collect()
    }

    /// Get decisions for analysis (e.g., finding patterns)
    pub fn get_by_context(&self, context_pattern: &str) -> Vec<&DecisionRecord> {
        self.decisions
            .iter()
            .filter(|d| d.context.contains(context_pattern))
            .collect()
    }

    /// Get success rate for a particular type of decision
    pub fn success_rate(&self, context_pattern: &str) -> f64 {
        let relevant: Vec<_> = self
            .decisions
            .iter()
            .filter(|d| d.context.contains(context_pattern) && d.outcome.is_some())
            .collect();

        if relevant.is_empty() {
            return 0.0;
        }

        let successes = relevant
            .iter()
            .filter(|d| {
                d.outcome
                    .as_ref()
                    .map_or(false, |o| o.contains("success") || o.contains("ok"))
            })
            .count() as f64;

        successes / relevant.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_and_retrieve() {
        let mut logger = DecisionLogger::new();
        let id = logger.log_decision(
            "route_task",
            &["agent-1".into(), "agent-2".into()],
            "agent-1",
            "Agent-1 has matching capabilities and is idle",
            "operational",
            "heuristic",
        );

        assert!(!id.is_empty());
        assert_eq!(logger.recent(1).len(), 1);
    }

    #[test]
    fn test_update_outcome() {
        let mut logger = DecisionLogger::new();
        let id = logger.log_decision(
            "route_task",
            &["agent-1".into()],
            "agent-1",
            "Only candidate",
            "reactive",
            "heuristic",
        );

        logger.update_outcome(&id, "success: task completed in 50ms");

        let decisions = logger.recent(1);
        assert_eq!(
            decisions[0].outcome.as_deref(),
            Some("success: task completed in 50ms")
        );
    }

    #[test]
    fn test_recent_returns_newest_first() {
        let mut logger = DecisionLogger::new();
        let id1 = logger.log_decision(
            "ctx_1",
            &["a".into()],
            "a",
            "first decision",
            "reactive",
            "heuristic",
        );
        let id2 = logger.log_decision(
            "ctx_2",
            &["b".into()],
            "b",
            "second decision",
            "reactive",
            "heuristic",
        );

        let recent = logger.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].id, id2); // newest first
        assert_eq!(recent[1].id, id1);
    }

    #[test]
    fn test_recent_limited_count() {
        let mut logger = DecisionLogger::new();
        for i in 0..10 {
            logger.log_decision(
                &format!("ctx_{i}"),
                &[],
                "agent",
                "reason",
                "reactive",
                "heuristic",
            );
        }

        let recent = logger.recent(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_get_by_context() {
        let mut logger = DecisionLogger::new();
        logger.log_decision(
            "route_task",
            &["a".into()],
            "a",
            "reason",
            "reactive",
            "heuristic",
        );
        logger.log_decision(
            "select_model",
            &["b".into()],
            "b",
            "reason",
            "tactical",
            "tinyllama",
        );
        logger.log_decision(
            "route_task",
            &["c".into()],
            "c",
            "reason",
            "operational",
            "heuristic",
        );

        let route_decisions = logger.get_by_context("route_task");
        assert_eq!(route_decisions.len(), 2);

        let model_decisions = logger.get_by_context("select_model");
        assert_eq!(model_decisions.len(), 1);

        let no_decisions = logger.get_by_context("nonexistent");
        assert_eq!(no_decisions.len(), 0);
    }

    #[test]
    fn test_success_rate() {
        let mut logger = DecisionLogger::new();

        // Log 4 decisions with outcomes
        for i in 0..4 {
            let id = logger.log_decision(
                "route_task",
                &["a".into()],
                "a",
                "reason",
                "reactive",
                "heuristic",
            );
            if i < 3 {
                logger.update_outcome(&id, "success");
            } else {
                logger.update_outcome(&id, "failed");
            }
        }

        let rate = logger.success_rate("route_task");
        assert!((rate - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_no_outcomes() {
        let mut logger = DecisionLogger::new();
        logger.log_decision(
            "route_task",
            &["a".into()],
            "a",
            "reason",
            "reactive",
            "heuristic",
        );
        // No outcomes set
        let rate = logger.success_rate("route_task");
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_success_rate_no_matching_context() {
        let logger = DecisionLogger::new();
        let rate = logger.success_rate("nonexistent");
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_success_rate_ok_outcome() {
        let mut logger = DecisionLogger::new();
        let id = logger.log_decision(
            "deploy",
            &["a".into()],
            "a",
            "reason",
            "tactical",
            "mistral",
        );
        logger.update_outcome(&id, "ok: deployed");

        let rate = logger.success_rate("deploy");
        assert_eq!(rate, 1.0);
    }

    #[test]
    fn test_update_outcome_nonexistent() {
        let mut logger = DecisionLogger::new();
        // Should not panic
        logger.update_outcome("nonexistent-id", "success");
    }

    #[test]
    fn test_capacity_trimming() {
        let mut logger = DecisionLogger::new();
        // The default max_entries is 10000
        // Push slightly more than that
        for i in 0..10005 {
            logger.log_decision(
                &format!("ctx_{i}"),
                &[],
                "agent",
                "reason",
                "reactive",
                "heuristic",
            );
        }

        // Should have exactly max_entries
        assert!(logger.decisions.len() <= 10000);
    }

    #[test]
    fn test_decision_record_fields() {
        let mut logger = DecisionLogger::new();
        let id = logger.log_decision(
            "task_routing",
            &["agent-1".into(), "agent-2".into(), "agent-3".into()],
            "agent-2",
            "Agent-2 has the best capabilities",
            "operational",
            "tinyllama",
        );

        let decisions = logger.recent(1);
        let d = decisions[0];
        assert_eq!(d.id, id);
        assert_eq!(d.context, "task_routing");
        assert_eq!(d.options.len(), 3);
        assert_eq!(d.chosen, "agent-2");
        assert_eq!(d.reasoning, "Agent-2 has the best capabilities");
        assert_eq!(d.intelligence_level, "operational");
        assert_eq!(d.model_used, "tinyllama");
        assert!(d.outcome.is_none());
        assert!(d.timestamp > 0);
    }
}
