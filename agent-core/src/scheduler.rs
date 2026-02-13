//! Cron-Like Scheduled Goals
//!
//! Evaluates cron expressions on a 60-second tick and creates goals when due.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// A scheduled goal entry
#[derive(Debug, Clone)]
pub struct ScheduledGoal {
    pub id: String,
    pub cron_expr: String,
    pub goal_template: String,
    pub priority: i32,
    pub enabled: bool,
    pub last_run: Option<i64>,
}

/// Goal scheduler with cron expression evaluation
pub struct GoalScheduler {
    pub schedules: HashMap<String, ScheduledGoal>,
    db_path: String,
}

impl GoalScheduler {
    pub fn new(db_path: &str) -> Self {
        Self {
            schedules: HashMap::new(),
            db_path: db_path.to_string(),
        }
    }

    /// Initialize database and load schedules
    pub fn load(&mut self) -> Result<()> {
        let conn =
            rusqlite::Connection::open(&self.db_path).context("Failed to open scheduler DB")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scheduled_goals (
                id TEXT PRIMARY KEY,
                cron_expr TEXT NOT NULL,
                goal_template TEXT NOT NULL,
                priority INTEGER DEFAULT 5,
                enabled INTEGER DEFAULT 1,
                last_run INTEGER
            )",
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, cron_expr, goal_template, priority, enabled, last_run FROM scheduled_goals",
        )?;

        let schedules: Vec<ScheduledGoal> = stmt
            .query_map([], |row| {
                Ok(ScheduledGoal {
                    id: row.get(0)?,
                    cron_expr: row.get(1)?,
                    goal_template: row.get(2)?,
                    priority: row.get(3)?,
                    enabled: row.get::<_, i32>(4)? != 0,
                    last_run: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        for schedule in schedules {
            self.schedules.insert(schedule.id.clone(), schedule);
        }

        info!("Loaded {} scheduled goals", self.schedules.len());
        Ok(())
    }

    /// Add a new schedule
    pub fn add_schedule(&mut self, schedule: ScheduledGoal) -> Result<()> {
        let conn = rusqlite::Connection::open(&self.db_path)?;
        conn.execute(
            "INSERT OR REPLACE INTO scheduled_goals (id, cron_expr, goal_template, priority, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![schedule.id, schedule.cron_expr, schedule.goal_template, schedule.priority, schedule.enabled as i32],
        )?;
        self.schedules.insert(schedule.id.clone(), schedule);
        Ok(())
    }

    /// Remove a schedule
    pub fn remove_schedule(&mut self, id: &str) -> Result<()> {
        let conn = rusqlite::Connection::open(&self.db_path)?;
        conn.execute("DELETE FROM scheduled_goals WHERE id = ?1", [id])?;
        self.schedules.remove(id);
        Ok(())
    }

    /// List all schedules
    pub fn list_schedules(&self) -> Vec<&ScheduledGoal> {
        self.schedules.values().collect()
    }

    /// Check which schedules are due
    pub fn check_due(&self, now: &chrono::DateTime<chrono::Utc>) -> Vec<&ScheduledGoal> {
        self.schedules
            .values()
            .filter(|s| {
                if !s.enabled {
                    return false;
                }
                // Don't fire more than once per minute
                if let Some(last) = s.last_run {
                    if now.timestamp() - last < 60 {
                        return false;
                    }
                }
                matches_cron(&s.cron_expr, now)
            })
            .collect()
    }

    /// Mark a schedule as having run
    pub fn mark_run(&mut self, id: &str, timestamp: i64) {
        if let Some(schedule) = self.schedules.get_mut(id) {
            schedule.last_run = Some(timestamp);
            if let Ok(conn) = rusqlite::Connection::open(&self.db_path) {
                conn.execute(
                    "UPDATE scheduled_goals SET last_run = ?1 WHERE id = ?2",
                    rusqlite::params![timestamp, id],
                )
                .ok();
            }
        }
    }

    /// Run the scheduler loop
    pub async fn run(
        scheduler: Arc<RwLock<Self>>,
        state: Arc<RwLock<crate::OrchestratorState>>,
        cancel: CancellationToken,
    ) {
        info!("Goal scheduler started");
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Goal scheduler shutting down");
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                    let now = chrono::Utc::now();
                    let due_ids: Vec<(String, String, i32)> = {
                        let sched = scheduler.read().await;
                        sched.check_due(&now)
                            .iter()
                            .map(|s| (s.id.clone(), s.goal_template.clone(), s.priority))
                            .collect()
                    };

                    for (id, goal_template, priority) in due_ids {
                        info!("Scheduled goal due: {}", &goal_template[..60.min(goal_template.len())]);
                        let mut state_w = state.write().await;
                        match state_w.goal_engine.submit_goal(
                            goal_template.clone(),
                            priority,
                            format!("scheduler:{id}"),
                        ).await {
                            Ok(goal_id) => {
                                if let Ok(tasks) = state_w.task_planner.decompose_goal(&goal_id, &goal_template).await {
                                    state_w.goal_engine.add_tasks(&goal_id, tasks);
                                }
                                drop(state_w);
                                let mut sched = scheduler.write().await;
                                sched.mark_run(&id, now.timestamp());
                            }
                            Err(e) => {
                                warn!("Failed to create scheduled goal: {e}");
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Simple cron expression matcher (minute hour day month weekday)
fn matches_cron(expression: &str, now: &chrono::DateTime<chrono::Utc>) -> bool {
    use chrono::Datelike;
    use chrono::Timelike;

    let parts: Vec<&str> = expression.split_whitespace().collect();
    if parts.len() != 5 {
        return false;
    }

    let checks = [
        (parts[0], now.minute()),
        (parts[1], now.hour()),
        (parts[2], now.day()),
        (parts[3], now.month()),
        (parts[4], now.weekday().num_days_from_monday() + 1),
    ];

    checks.iter().all(|(pattern, value)| matches_field(pattern, *value))
}

fn matches_field(pattern: &str, value: u32) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(interval_str) = pattern.strip_prefix("*/") {
        if let Ok(interval) = interval_str.parse::<u32>() {
            return interval > 0 && value % interval == 0;
        }
    }
    for part in pattern.split(',') {
        if let Ok(n) = part.trim().parse::<u32>() {
            if n == value {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_field_wildcard() {
        assert!(matches_field("*", 5));
        assert!(matches_field("*", 0));
    }

    #[test]
    fn test_matches_field_interval() {
        assert!(matches_field("*/5", 0));
        assert!(matches_field("*/5", 5));
        assert!(!matches_field("*/5", 3));
    }

    #[test]
    fn test_matches_field_specific() {
        assert!(matches_field("10", 10));
        assert!(!matches_field("10", 5));
    }

    #[test]
    fn test_goal_scheduler_new() {
        let scheduler = GoalScheduler::new("/tmp/test_scheduler.db");
        assert!(scheduler.schedules.is_empty());
    }
}
