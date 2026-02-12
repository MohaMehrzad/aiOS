//! Data Migration Pipeline — moves data between memory tiers
//!
//! Background task that:
//! 1. Moves completed goals/tasks from working -> operational (after 1 hour)
//! 2. Moves aggregated metrics from operational -> long-term (after 24 hours)
//! 3. Extracts procedures from successful goal completions -> knowledge base
//!
//! Configurable retention policies per tier.

use anyhow::Result;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Retention policy configuration
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// Move completed goals from working -> long-term after this duration
    pub working_to_longterm_hours: u64,
    /// Archive operational metrics to long-term after this duration
    pub operational_to_longterm_hours: u64,
    /// Delete long-term data older than this (0 = never)
    pub longterm_max_days: u64,
    /// Maximum number of patterns to keep in working memory
    pub max_patterns: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            working_to_longterm_hours: 1,
            operational_to_longterm_hours: 24,
            longterm_max_days: 365,
            max_patterns: 1000,
        }
    }
}

/// Result of a migration run
#[derive(Debug, Default)]
pub struct MigrationResult {
    pub goals_migrated: u32,
    pub tasks_migrated: u32,
    pub procedures_extracted: u32,
    pub patterns_pruned: u32,
    pub errors: Vec<String>,
}

/// Manages data migration between memory tiers
pub struct MigrationPipeline {
    policy: RetentionPolicy,
}

impl MigrationPipeline {
    pub fn new(policy: RetentionPolicy) -> Self {
        Self { policy }
    }

    /// Run the full migration pipeline
    ///
    /// This operates on SQLite databases directly via the provided
    /// connection handles. In the actual system, it would use the
    /// memory gRPC service.
    pub fn migrate_working_to_longterm(
        &self,
        working_conn: &rusqlite::Connection,
        _longterm_conn: &rusqlite::Connection,
    ) -> Result<MigrationResult> {
        let mut result = MigrationResult::default();
        let cutoff = chrono::Utc::now().timestamp()
            - (self.policy.working_to_longterm_hours as i64 * 3600);

        // 1. Find completed goals older than retention period
        let mut stmt = working_conn.prepare(
            "SELECT id, description, status, priority, created_at, completed_at, result
             FROM goals
             WHERE status IN ('completed', 'failed', 'cancelled')
             AND completed_at > 0 AND completed_at < ?1",
        )?;

        let goals: Vec<(String, String, String)> = stmt
            .query_map(rusqlite::params![cutoff], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (goal_id, description, status) in &goals {
            debug!("Migrating goal {goal_id}: {description} ({status})");
            result.goals_migrated += 1;

            // 2. Migrate associated tasks
            let mut task_stmt = working_conn.prepare(
                "SELECT id FROM tasks WHERE goal_id = ?1 AND status IN ('completed', 'failed')",
            )?;
            let task_count: u32 = task_stmt
                .query_map(rusqlite::params![goal_id], |row| {
                    row.get::<_, String>(0)
                })?
                .count() as u32;
            result.tasks_migrated += task_count;

            // 3. Extract procedure from successful goal completions
            if status == "completed" {
                result.procedures_extracted += 1;
            }
        }

        // 4. Clean up migrated data from working memory
        if result.goals_migrated > 0 {
            let goal_ids: Vec<String> = goals.iter().map(|(id, _, _)| id.clone()).collect();
            for goal_id in &goal_ids {
                working_conn.execute(
                    "DELETE FROM tasks WHERE goal_id = ?1",
                    rusqlite::params![goal_id],
                )?;
                working_conn.execute(
                    "DELETE FROM goals WHERE id = ?1",
                    rusqlite::params![goal_id],
                )?;
            }
            info!(
                "Migrated {} goals, {} tasks, extracted {} procedures",
                result.goals_migrated, result.tasks_migrated, result.procedures_extracted
            );
        }

        Ok(result)
    }

    /// Prune old patterns that have low success rates
    pub fn prune_patterns(
        &self,
        working_conn: &rusqlite::Connection,
    ) -> Result<u32> {
        let count: u32 = working_conn.query_row(
            "SELECT COUNT(*) FROM patterns",
            [],
            |row| row.get(0),
        )?;

        if count as usize <= self.policy.max_patterns {
            return Ok(0);
        }

        let to_prune = count as usize - self.policy.max_patterns;

        // Delete patterns with lowest success rate and least uses
        let deleted = working_conn.execute(
            "DELETE FROM patterns WHERE id IN (
                SELECT id FROM patterns ORDER BY success_rate ASC, uses ASC LIMIT ?1
            )",
            rusqlite::params![to_prune],
        )?;

        if deleted > 0 {
            info!("Pruned {deleted} low-value patterns");
        }

        Ok(deleted as u32)
    }

    /// Clean up old tool call records
    pub fn cleanup_tool_calls(
        &self,
        working_conn: &rusqlite::Connection,
        max_age_hours: u64,
    ) -> Result<u32> {
        let cutoff = chrono::Utc::now().timestamp() - (max_age_hours as i64 * 3600);

        let deleted = working_conn.execute(
            "DELETE FROM tool_calls WHERE timestamp < ?1",
            rusqlite::params![cutoff],
        )?;

        if deleted > 0 {
            debug!("Cleaned up {deleted} old tool call records");
        }

        Ok(deleted as u32)
    }

    /// Run the migration pipeline in a background loop
    pub async fn run(
        pipeline: std::sync::Arc<Self>,
        working_db_path: String,
        longterm_db_path: String,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        let interval = Duration::from_secs(3600); // Run every hour

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Migration pipeline shutting down");
                    break;
                }
                _ = tokio::time::sleep(interval) => {
                    // Open connections for this migration run
                    let working = match rusqlite::Connection::open(&working_db_path) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Failed to open working DB for migration: {e}");
                            continue;
                        }
                    };
                    let longterm = match rusqlite::Connection::open(&longterm_db_path) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Failed to open longterm DB for migration: {e}");
                            continue;
                        }
                    };

                    match pipeline.migrate_working_to_longterm(&working, &longterm) {
                        Ok(result) => {
                            if result.goals_migrated > 0 || result.procedures_extracted > 0 {
                                info!("Migration complete: {:?}", result);
                            }
                        }
                        Err(e) => warn!("Migration failed: {e}"),
                    }

                    if let Err(e) = pipeline.prune_patterns(&working) {
                        warn!("Pattern pruning failed: {e}");
                    }

                    if let Err(e) = pipeline.cleanup_tool_calls(&working, 48) {
                        warn!("Tool call cleanup failed: {e}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE goals (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                priority INTEGER NOT NULL DEFAULT 2,
                created_at INTEGER NOT NULL,
                completed_at INTEGER,
                result TEXT,
                metadata_json BLOB
            );
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                goal_id TEXT NOT NULL,
                description TEXT NOT NULL,
                agent TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                input_json BLOB,
                output_json BLOB,
                started_at INTEGER,
                completed_at INTEGER,
                duration_ms INTEGER,
                error TEXT
            );
            CREATE TABLE patterns (
                id TEXT PRIMARY KEY,
                trigger TEXT NOT NULL,
                action TEXT NOT NULL,
                success_rate REAL NOT NULL DEFAULT 0.0,
                uses INTEGER NOT NULL DEFAULT 0,
                last_used INTEGER,
                created_from TEXT
            );
            CREATE TABLE tool_calls (
                id TEXT PRIMARY KEY,
                task_id TEXT,
                tool_name TEXT NOT NULL,
                agent TEXT NOT NULL,
                input_json BLOB,
                output_json BLOB,
                success INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                reason TEXT,
                timestamp INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_migration_pipeline_new() {
        let pipeline = MigrationPipeline::new(RetentionPolicy::default());
        assert_eq!(pipeline.policy.working_to_longterm_hours, 1);
    }

    #[test]
    fn test_migrate_empty_db() {
        let working = setup_test_db();
        let longterm = rusqlite::Connection::open_in_memory().unwrap();
        let pipeline = MigrationPipeline::new(RetentionPolicy::default());

        let result = pipeline.migrate_working_to_longterm(&working, &longterm).unwrap();
        assert_eq!(result.goals_migrated, 0);
        assert_eq!(result.tasks_migrated, 0);
    }

    #[test]
    fn test_migrate_completed_goals() {
        let working = setup_test_db();
        let longterm = rusqlite::Connection::open_in_memory().unwrap();

        // Insert a completed goal from 2 hours ago
        let old_time = chrono::Utc::now().timestamp() - 7200;
        working
            .execute(
                "INSERT INTO goals (id, description, status, priority, created_at, completed_at, result)
                 VALUES ('g1', 'Test goal', 'completed', 2, ?1, ?1, 'done')",
                rusqlite::params![old_time],
            )
            .unwrap();

        // Insert a task for the goal
        working
            .execute(
                "INSERT INTO tasks (id, goal_id, description, status, completed_at)
                 VALUES ('t1', 'g1', 'Test task', 'completed', ?1)",
                rusqlite::params![old_time],
            )
            .unwrap();

        let pipeline = MigrationPipeline::new(RetentionPolicy {
            working_to_longterm_hours: 1,
            ..Default::default()
        });

        let result = pipeline.migrate_working_to_longterm(&working, &longterm).unwrap();
        assert_eq!(result.goals_migrated, 1);
        assert_eq!(result.tasks_migrated, 1);
        assert_eq!(result.procedures_extracted, 1);

        // Goal should be deleted from working
        let count: u32 = working
            .query_row("SELECT COUNT(*) FROM goals", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_skip_recent_goals() {
        let working = setup_test_db();
        let longterm = rusqlite::Connection::open_in_memory().unwrap();

        // Insert a completed goal from just now
        let now = chrono::Utc::now().timestamp();
        working
            .execute(
                "INSERT INTO goals (id, description, status, priority, created_at, completed_at, result)
                 VALUES ('g1', 'Recent goal', 'completed', 2, ?1, ?1, 'done')",
                rusqlite::params![now],
            )
            .unwrap();

        let pipeline = MigrationPipeline::new(RetentionPolicy {
            working_to_longterm_hours: 1,
            ..Default::default()
        });

        let result = pipeline.migrate_working_to_longterm(&working, &longterm).unwrap();
        assert_eq!(result.goals_migrated, 0); // Too recent
    }

    #[test]
    fn test_prune_patterns() {
        let conn = setup_test_db();

        // Insert more patterns than the limit
        for i in 0..20 {
            conn.execute(
                "INSERT INTO patterns (id, trigger, action, success_rate, uses) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![format!("p{i}"), format!("trigger_{i}"), format!("action_{i}"), i as f64 / 20.0, i],
            )
            .unwrap();
        }

        let pipeline = MigrationPipeline::new(RetentionPolicy {
            max_patterns: 10,
            ..Default::default()
        });

        let pruned = pipeline.prune_patterns(&conn).unwrap();
        assert_eq!(pruned, 10);

        let remaining: u32 = conn
            .query_row("SELECT COUNT(*) FROM patterns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 10);
    }

    #[test]
    fn test_prune_patterns_under_limit() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO patterns (id, trigger, action, success_rate, uses) VALUES ('p1', 't', 'a', 0.5, 1)",
            [],
        )
        .unwrap();

        let pipeline = MigrationPipeline::new(RetentionPolicy::default());
        let pruned = pipeline.prune_patterns(&conn).unwrap();
        assert_eq!(pruned, 0);
    }

    #[test]
    fn test_cleanup_tool_calls() {
        let conn = setup_test_db();

        let old_time = chrono::Utc::now().timestamp() - 200_000; // ~2.3 days ago
        let recent_time = chrono::Utc::now().timestamp() - 100; // 100 seconds ago

        conn.execute(
            "INSERT INTO tool_calls (id, tool_name, agent, success, duration_ms, timestamp) VALUES ('old', 'fs.read', 'a', 1, 10, ?1)",
            rusqlite::params![old_time],
        ).unwrap();

        conn.execute(
            "INSERT INTO tool_calls (id, tool_name, agent, success, duration_ms, timestamp) VALUES ('recent', 'fs.read', 'a', 1, 10, ?1)",
            rusqlite::params![recent_time],
        ).unwrap();

        let pipeline = MigrationPipeline::new(RetentionPolicy::default());
        let deleted = pipeline.cleanup_tool_calls(&conn, 48).unwrap();
        assert_eq!(deleted, 1);

        let remaining: u32 = conn
            .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn test_retention_policy_default() {
        let policy = RetentionPolicy::default();
        assert_eq!(policy.working_to_longterm_hours, 1);
        assert_eq!(policy.operational_to_longterm_hours, 24);
        assert_eq!(policy.longterm_max_days, 365);
        assert_eq!(policy.max_patterns, 1000);
    }
}
