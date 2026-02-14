//! Working Memory — SQLite-backed warm storage
//!
//! Stores goals, tasks, tool calls, decisions, patterns, and agent state.
//! Retention: 30 days default, then migrated to long-term.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::proto::memory::*;

/// SQLite-backed working memory
pub struct WorkingMemory {
    conn: Mutex<Connection>,
}

impl WorkingMemory {
    pub fn new(db_path: &str) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        // Create all tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS goals (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                priority INTEGER NOT NULL DEFAULT 2,
                created_at INTEGER NOT NULL,
                completed_at INTEGER,
                result TEXT,
                metadata_json BLOB
            );

            CREATE TABLE IF NOT EXISTS tasks (
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
                error TEXT,
                FOREIGN KEY (goal_id) REFERENCES goals(id)
            );

            CREATE TABLE IF NOT EXISTS tool_calls (
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
            );

            CREATE TABLE IF NOT EXISTS decisions (
                id TEXT PRIMARY KEY,
                context TEXT NOT NULL,
                options_json BLOB,
                chosen TEXT NOT NULL,
                reasoning TEXT NOT NULL,
                intelligence_level TEXT NOT NULL,
                model_used TEXT NOT NULL,
                outcome TEXT,
                timestamp INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS patterns (
                id TEXT PRIMARY KEY,
                trigger TEXT NOT NULL,
                action TEXT NOT NULL,
                success_rate REAL NOT NULL DEFAULT 0.0,
                uses INTEGER NOT NULL DEFAULT 0,
                last_used INTEGER,
                created_from TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_states (
                agent_name TEXT PRIMARY KEY,
                state_json BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
            CREATE INDEX IF NOT EXISTS idx_tasks_goal ON tasks(goal_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_task ON tool_calls(task_id);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_tool ON tool_calls(tool_name);
            CREATE INDEX IF NOT EXISTS idx_decisions_context ON decisions(context);
            CREATE INDEX IF NOT EXISTS idx_patterns_trigger ON patterns(trigger);",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // --- Goals ---

    pub fn store_goal(&self, goal: &GoalRecord) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO goals (id, description, status, priority, created_at, completed_at, result, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                goal.id,
                goal.description,
                goal.status,
                goal.priority,
                goal.created_at,
                goal.completed_at,
                goal.result,
                goal.metadata_json,
            ],
        )?;
        Ok(())
    }

    pub fn update_goal(&self, update: &GoalUpdate) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "UPDATE goals SET status = ?1, result = ?2 WHERE id = ?3",
            params![update.status, update.result, update.id],
        )?;
        Ok(())
    }

    pub fn get_active_goals(&self) -> Result<Vec<GoalRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, description, status, priority, created_at, completed_at, result, metadata_json
             FROM goals WHERE status NOT IN ('completed', 'failed', 'cancelled')
             ORDER BY priority ASC, created_at ASC",
        )?;

        let goals = stmt
            .query_map([], |row| {
                Ok(GoalRecord {
                    id: row.get(0)?,
                    description: row.get(1)?,
                    status: row.get(2)?,
                    priority: row.get(3)?,
                    created_at: row.get(4)?,
                    completed_at: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    result: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    metadata_json: row.get::<_, Option<Vec<u8>>>(7)?.unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(goals)
    }

    // --- Tasks ---

    pub fn store_task(&self, task: &TaskRecord) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO tasks (id, goal_id, description, agent, status, input_json, output_json, started_at, completed_at, duration_ms, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                task.id,
                task.goal_id,
                task.description,
                task.agent,
                task.status,
                task.input_json,
                task.output_json,
                task.started_at,
                task.completed_at,
                task.duration_ms,
                task.error,
            ],
        )?;
        Ok(())
    }

    pub fn get_tasks_for_goal(&self, goal_id: &str) -> Result<Vec<TaskRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, goal_id, description, agent, status, input_json, output_json, started_at, completed_at, duration_ms, error
             FROM tasks WHERE goal_id = ?1 ORDER BY started_at ASC",
        )?;

        let tasks = stmt
            .query_map(params![goal_id], |row| {
                Ok(TaskRecord {
                    id: row.get(0)?,
                    goal_id: row.get(1)?,
                    description: row.get(2)?,
                    agent: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    status: row.get(4)?,
                    input_json: row.get::<_, Option<Vec<u8>>>(5)?.unwrap_or_default(),
                    output_json: row.get::<_, Option<Vec<u8>>>(6)?.unwrap_or_default(),
                    started_at: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    completed_at: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    duration_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    error: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tasks)
    }

    // --- Tool Calls ---

    pub fn store_tool_call(&self, record: &ToolCallRecord) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT INTO tool_calls (id, task_id, tool_name, agent, input_json, output_json, success, duration_ms, reason, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.id,
                record.task_id,
                record.tool_name,
                record.agent,
                record.input_json,
                record.output_json,
                record.success as i32,
                record.duration_ms,
                record.reason,
                record.timestamp,
            ],
        )?;
        Ok(())
    }

    // --- Decisions ---

    pub fn store_decision(&self, decision: &Decision) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT INTO decisions (id, context, options_json, chosen, reasoning, intelligence_level, model_used, outcome, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                decision.id,
                decision.context,
                decision.options_json,
                decision.chosen,
                decision.reasoning,
                decision.intelligence_level,
                decision.model_used,
                decision.outcome,
                decision.timestamp,
            ],
        )?;
        Ok(())
    }

    // --- Patterns ---

    pub fn store_pattern(&self, pattern: &Pattern) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO patterns (id, trigger, action, success_rate, uses, last_used, created_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                pattern.id,
                pattern.trigger,
                pattern.action,
                pattern.success_rate,
                pattern.uses,
                pattern.last_used,
                pattern.created_from,
            ],
        )?;
        Ok(())
    }

    pub fn find_pattern(&self, trigger: &str, min_success_rate: f64) -> Result<PatternResult> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let result = conn.query_row(
            "SELECT id, trigger, action, success_rate, uses, last_used, created_from
             FROM patterns WHERE trigger LIKE ?1 AND success_rate >= ?2
             ORDER BY success_rate DESC, uses DESC LIMIT 1",
            params![format!("%{trigger}%"), min_success_rate],
            |row| {
                Ok(Pattern {
                    id: row.get(0)?,
                    trigger: row.get(1)?,
                    action: row.get(2)?,
                    success_rate: row.get(3)?,
                    uses: row.get(4)?,
                    last_used: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    created_from: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                })
            },
        );

        match result {
            Ok(pattern) => Ok(PatternResult {
                pattern: Some(pattern),
                found: true,
            }),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(PatternResult {
                pattern: None,
                found: false,
            }),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_pattern_stats(&self, id: &str, success: bool) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let now = chrono::Utc::now().timestamp();

        // Update uses count and recalculate success rate
        if success {
            conn.execute(
                "UPDATE patterns SET
                    uses = uses + 1,
                    last_used = ?1,
                    success_rate = (success_rate * uses + 1.0) / (uses + 1)
                 WHERE id = ?2",
                params![now, id],
            )?;
        } else {
            conn.execute(
                "UPDATE patterns SET
                    uses = uses + 1,
                    last_used = ?1,
                    success_rate = (success_rate * uses) / (uses + 1)
                 WHERE id = ?2",
                params![now, id],
            )?;
        }

        Ok(())
    }

    // --- Pattern Learning ---

    /// Extract and store a pattern from a successful task completion
    /// Called after a goal is completed successfully to learn from the outcome
    pub fn learn_pattern_from_goal(
        &self,
        goal_description: &str,
        tool_sequence: &[String],
        goal_id: &str,
    ) -> Result<()> {
        if tool_sequence.is_empty() {
            return Ok(());
        }

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let pattern_id = uuid::Uuid::new_v4().to_string();
        let action = tool_sequence.join(" → ");
        let now = chrono::Utc::now().timestamp();

        // Check if a similar pattern already exists
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM patterns WHERE trigger LIKE ?1 AND action = ?2 LIMIT 1",
                params![
                    format!("%{}%", &goal_description[..goal_description.len().min(50)]),
                    &action
                ],
                |row| row.get(0),
            )
            .ok();

        if let Some(existing_id) = existing {
            // Update existing pattern's success stats
            conn.execute(
                "UPDATE patterns SET uses = uses + 1, last_used = ?1,
                 success_rate = (success_rate * uses + 1.0) / (uses + 1)
                 WHERE id = ?2",
                params![now, existing_id],
            )?;
        } else {
            // Insert new pattern
            conn.execute(
                "INSERT INTO patterns (id, trigger, action, success_rate, uses, last_used, created_from)
                 VALUES (?1, ?2, ?3, 1.0, 1, ?4, ?5)",
                params![pattern_id, goal_description, action, now, goal_id],
            )?;
        }

        Ok(())
    }

    /// Get tool sequence used for a completed goal
    pub fn get_tool_sequence_for_goal(&self, goal_id: &str) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT tool_name FROM tool_calls
             WHERE task_id IN (SELECT id FROM tasks WHERE goal_id = ?1)
             ORDER BY timestamp ASC",
        )?;

        let tools: Vec<String> = stmt
            .query_map(params![goal_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tools)
    }

    // --- Agent State ---

    pub fn store_agent_state(&self, state: &AgentState) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO agent_states (agent_name, state_json, updated_at)
             VALUES (?1, ?2, ?3)",
            params![state.agent_name, state.state_json, state.updated_at],
        )?;
        Ok(())
    }

    pub fn get_agent_state(&self, agent_name: &str) -> Result<AgentState> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let state = conn.query_row(
            "SELECT agent_name, state_json, updated_at FROM agent_states WHERE agent_name = ?1",
            params![agent_name],
            |row| {
                Ok(AgentState {
                    agent_name: row.get(0)?,
                    state_json: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            },
        )?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> WorkingMemory {
        WorkingMemory::new(":memory:").unwrap()
    }

    #[test]
    fn test_goal_lifecycle() {
        let wm = test_db();
        let goal = GoalRecord {
            id: "goal-1".into(),
            description: "Test goal".into(),
            status: "pending".into(),
            priority: 2,
            created_at: 1000,
            completed_at: 0,
            result: String::new(),
            metadata_json: vec![],
        };

        wm.store_goal(&goal).unwrap();
        let goals = wm.get_active_goals().unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].description, "Test goal");

        wm.update_goal(&GoalUpdate {
            id: "goal-1".into(),
            status: "completed".into(),
            result: "done".into(),
        })
        .unwrap();

        let goals = wm.get_active_goals().unwrap();
        assert_eq!(goals.len(), 0);
    }

    #[test]
    fn test_pattern_matching() {
        let wm = test_db();
        wm.store_pattern(&Pattern {
            id: "p1".into(),
            trigger: "high cpu usage".into(),
            action: "restart heavy service".into(),
            success_rate: 0.9,
            uses: 10,
            last_used: 0,
            created_from: "incident-1".into(),
        })
        .unwrap();

        let result = wm.find_pattern("cpu", 0.5).unwrap();
        assert!(result.found);
        assert_eq!(result.pattern.unwrap().action, "restart heavy service");

        let result = wm.find_pattern("disk", 0.5).unwrap();
        assert!(!result.found);
    }

    #[test]
    fn test_store_and_retrieve_task() {
        let wm = test_db();

        // Store a goal first (foreign key)
        wm.store_goal(&GoalRecord {
            id: "goal-1".into(),
            description: "Test goal".into(),
            status: "pending".into(),
            priority: 1,
            created_at: 1000,
            completed_at: 0,
            result: String::new(),
            metadata_json: vec![],
        })
        .unwrap();

        let task = TaskRecord {
            id: "task-1".into(),
            goal_id: "goal-1".into(),
            description: "Do something".into(),
            agent: "agent-1".into(),
            status: "pending".into(),
            input_json: b"{}".to_vec(),
            output_json: vec![],
            started_at: 1000,
            completed_at: 0,
            duration_ms: 0,
            error: String::new(),
        };

        wm.store_task(&task).unwrap();

        let tasks = wm.get_tasks_for_goal("goal-1").unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description, "Do something");
        assert_eq!(tasks[0].agent, "agent-1");
    }

    #[test]
    fn test_get_tasks_for_nonexistent_goal() {
        let wm = test_db();
        let tasks = wm.get_tasks_for_goal("nonexistent").unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_store_tool_call() {
        let wm = test_db();
        let record = ToolCallRecord {
            id: "tc-1".into(),
            task_id: "task-1".into(),
            tool_name: "fs.read".into(),
            agent: "agent-1".into(),
            input_json: b"{\"path\":\"/etc/hosts\"}".to_vec(),
            output_json: b"{\"content\":\"...\"}".to_vec(),
            success: true,
            duration_ms: 50,
            reason: "Need to read hosts file".into(),
            timestamp: 1000,
        };

        // Should not error
        wm.store_tool_call(&record).unwrap();
    }

    #[test]
    fn test_store_decision() {
        let wm = test_db();
        let decision = Decision {
            id: "d-1".into(),
            context: "route_task".into(),
            options_json: b"[\"agent-1\",\"agent-2\"]".to_vec(),
            chosen: "agent-1".into(),
            reasoning: "Agent-1 is idle".into(),
            intelligence_level: "reactive".into(),
            model_used: "heuristic".into(),
            outcome: "success".into(),
            timestamp: 1000,
        };

        wm.store_decision(&decision).unwrap();
    }

    #[test]
    fn test_pattern_with_high_min_success_rate() {
        let wm = test_db();
        wm.store_pattern(&Pattern {
            id: "p1".into(),
            trigger: "disk full".into(),
            action: "cleanup tmp".into(),
            success_rate: 0.5,
            uses: 4,
            last_used: 0,
            created_from: "".into(),
        })
        .unwrap();

        // min_success_rate too high
        let result = wm.find_pattern("disk", 0.9).unwrap();
        assert!(!result.found);

        // min_success_rate satisfied
        let result = wm.find_pattern("disk", 0.4).unwrap();
        assert!(result.found);
    }

    #[test]
    fn test_update_pattern_stats_success() {
        let wm = test_db();
        wm.store_pattern(&Pattern {
            id: "p1".into(),
            trigger: "high cpu".into(),
            action: "restart".into(),
            success_rate: 0.8,
            uses: 10,
            last_used: 0,
            created_from: "".into(),
        })
        .unwrap();

        wm.update_pattern_stats("p1", true).unwrap();

        let result = wm.find_pattern("cpu", 0.0).unwrap();
        assert!(result.found);
        let p = result.pattern.unwrap();
        assert_eq!(p.uses, 11);
        // success_rate should be recalculated: (0.8 * 10 + 1) / 11 = 9/11 ~= 0.818
        assert!(p.success_rate > 0.8);
    }

    #[test]
    fn test_update_pattern_stats_failure() {
        let wm = test_db();
        wm.store_pattern(&Pattern {
            id: "p1".into(),
            trigger: "high cpu".into(),
            action: "restart".into(),
            success_rate: 1.0,
            uses: 10,
            last_used: 0,
            created_from: "".into(),
        })
        .unwrap();

        wm.update_pattern_stats("p1", false).unwrap();

        let result = wm.find_pattern("cpu", 0.0).unwrap();
        let p = result.pattern.unwrap();
        assert_eq!(p.uses, 11);
        // success_rate should decrease: (1.0 * 10) / 11 ~= 0.909
        assert!(p.success_rate < 1.0);
    }

    #[test]
    fn test_agent_state_store_and_retrieve() {
        let wm = test_db();
        let state = AgentState {
            agent_name: "agent-1".into(),
            state_json: b"{\"status\":\"idle\"}".to_vec(),
            updated_at: 1000,
        };

        wm.store_agent_state(&state).unwrap();

        let retrieved = wm.get_agent_state("agent-1").unwrap();
        assert_eq!(retrieved.agent_name, "agent-1");
        assert_eq!(retrieved.state_json, b"{\"status\":\"idle\"}");
        assert_eq!(retrieved.updated_at, 1000);
    }

    #[test]
    fn test_agent_state_overwrite() {
        let wm = test_db();

        wm.store_agent_state(&AgentState {
            agent_name: "agent-1".into(),
            state_json: b"old".to_vec(),
            updated_at: 1000,
        })
        .unwrap();

        wm.store_agent_state(&AgentState {
            agent_name: "agent-1".into(),
            state_json: b"new".to_vec(),
            updated_at: 2000,
        })
        .unwrap();

        let retrieved = wm.get_agent_state("agent-1").unwrap();
        assert_eq!(retrieved.state_json, b"new");
        assert_eq!(retrieved.updated_at, 2000);
    }

    #[test]
    fn test_get_agent_state_nonexistent() {
        let wm = test_db();
        let result = wm.get_agent_state("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_goal_upsert() {
        let wm = test_db();
        let goal = GoalRecord {
            id: "goal-1".into(),
            description: "Original".into(),
            status: "pending".into(),
            priority: 1,
            created_at: 1000,
            completed_at: 0,
            result: String::new(),
            metadata_json: vec![],
        };
        wm.store_goal(&goal).unwrap();

        // Store same id with different description (INSERT OR REPLACE)
        let updated = GoalRecord {
            id: "goal-1".into(),
            description: "Updated".into(),
            status: "in_progress".into(),
            priority: 1,
            created_at: 1000,
            completed_at: 0,
            result: String::new(),
            metadata_json: vec![],
        };
        wm.store_goal(&updated).unwrap();

        let goals = wm.get_active_goals().unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].description, "Updated");
    }

    #[test]
    fn test_multiple_goals_ordering() {
        let wm = test_db();

        // Lower priority number = higher priority
        wm.store_goal(&GoalRecord {
            id: "g1".into(),
            description: "Low priority".into(),
            status: "pending".into(),
            priority: 5,
            created_at: 1000,
            completed_at: 0,
            result: String::new(),
            metadata_json: vec![],
        })
        .unwrap();

        wm.store_goal(&GoalRecord {
            id: "g2".into(),
            description: "High priority".into(),
            status: "pending".into(),
            priority: 1,
            created_at: 2000,
            completed_at: 0,
            result: String::new(),
            metadata_json: vec![],
        })
        .unwrap();

        let goals = wm.get_active_goals().unwrap();
        assert_eq!(goals.len(), 2);
        // Should be ordered by priority ASC
        assert_eq!(goals[0].priority, 1);
        assert_eq!(goals[1].priority, 5);
    }

    #[test]
    fn test_multiple_tasks_for_goal() {
        let wm = test_db();

        wm.store_goal(&GoalRecord {
            id: "goal-1".into(),
            description: "Test".into(),
            status: "pending".into(),
            priority: 1,
            created_at: 1000,
            completed_at: 0,
            result: String::new(),
            metadata_json: vec![],
        })
        .unwrap();

        for i in 0..5 {
            wm.store_task(&TaskRecord {
                id: format!("task-{i}"),
                goal_id: "goal-1".into(),
                description: format!("Task {i}"),
                agent: String::new(),
                status: "pending".into(),
                input_json: vec![],
                output_json: vec![],
                started_at: 1000 + i,
                completed_at: 0,
                duration_ms: 0,
                error: String::new(),
            })
            .unwrap();
        }

        let tasks = wm.get_tasks_for_goal("goal-1").unwrap();
        assert_eq!(tasks.len(), 5);
    }
}
