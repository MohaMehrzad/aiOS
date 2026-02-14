//! Goal Engine — manages the lifecycle of goals
//!
//! Goals flow through: Pending → Planning → InProgress → Completed/Failed
//!
//! Storage: HashMap in-memory cache + optional SQLite persistence.
//! When a db_path is provided, all mutations are written to SQLite so
//! goals, tasks, and messages survive service restarts.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use crate::proto::common::{Goal, Task};

/// A message in a goal's conversation thread
#[derive(Clone, Debug, serde::Serialize)]
pub struct GoalMessage {
    pub id: String,
    pub sender: String, // "user" | "ai" | "system"
    pub content: String,
    pub timestamp: i64,
}

/// Manages goals and their lifecycle
pub struct GoalEngine {
    goals: HashMap<String, Goal>,
    goal_tasks: HashMap<String, Vec<Task>>,
    goal_messages: HashMap<String, Vec<GoalMessage>>,
    /// Optional SQLite connection for persistence (Mutex because Connection is !Send)
    db: Option<Mutex<rusqlite::Connection>>,
}

impl GoalEngine {
    /// Create a new in-memory-only GoalEngine (for tests)
    pub fn new() -> Self {
        Self {
            goals: HashMap::new(),
            goal_tasks: HashMap::new(),
            goal_messages: HashMap::new(),
            db: None,
        }
    }

    /// Create a GoalEngine backed by SQLite at the given path.
    /// Creates the database and tables if they don't exist, then loads all
    /// existing data into the in-memory cache.
    pub fn with_db(db_path: &str) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = rusqlite::Connection::open(db_path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        // Create tables
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS goals (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                priority INTEGER NOT NULL,
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]',
                metadata_json BLOB NOT NULL DEFAULT X''
            );
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                goal_id TEXT NOT NULL,
                description TEXT NOT NULL,
                assigned_agent TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL,
                intelligence_level TEXT NOT NULL DEFAULT '',
                required_tools TEXT NOT NULL DEFAULT '[]',
                depends_on TEXT NOT NULL DEFAULT '[]',
                input_json BLOB NOT NULL DEFAULT X'',
                output_json BLOB NOT NULL DEFAULT X'',
                created_at INTEGER NOT NULL DEFAULT 0,
                started_at INTEGER NOT NULL DEFAULT 0,
                completed_at INTEGER NOT NULL DEFAULT 0,
                error TEXT NOT NULL DEFAULT '',
                FOREIGN KEY(goal_id) REFERENCES goals(id)
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                goal_id TEXT NOT NULL,
                sender TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                FOREIGN KEY(goal_id) REFERENCES goals(id)
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_goal ON tasks(goal_id);
            CREATE INDEX IF NOT EXISTS idx_messages_goal ON messages(goal_id);",
        )?;

        // Load existing data into cache
        let mut goals = HashMap::new();
        let mut goal_tasks: HashMap<String, Vec<Task>> = HashMap::new();
        let mut goal_messages: HashMap<String, Vec<GoalMessage>> = HashMap::new();

        // Load goals
        {
            let mut stmt = db.prepare(
                "SELECT id, description, priority, source, status, created_at, updated_at, tags, metadata_json FROM goals"
            )?;
            let rows = stmt.query_map([], |row| {
                let tags_json: String = row.get(7)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                Ok(Goal {
                    id: row.get(0)?,
                    description: row.get(1)?,
                    priority: row.get(2)?,
                    source: row.get(3)?,
                    status: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    tags,
                    metadata_json: row.get(8)?,
                })
            })?;
            for row in rows {
                let goal = row?;
                let id = goal.id.clone();
                goals.insert(id.clone(), goal);
                goal_tasks.entry(id).or_default();
            }
        }

        // Load tasks
        {
            let mut stmt = db.prepare(
                "SELECT id, goal_id, description, assigned_agent, status, intelligence_level, \
                 required_tools, depends_on, input_json, output_json, created_at, started_at, \
                 completed_at, error FROM tasks ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                let tools_json: String = row.get(6)?;
                let deps_json: String = row.get(7)?;
                Ok(Task {
                    id: row.get(0)?,
                    goal_id: row.get(1)?,
                    description: row.get(2)?,
                    assigned_agent: row.get(3)?,
                    status: row.get(4)?,
                    intelligence_level: row.get(5)?,
                    required_tools: serde_json::from_str(&tools_json).unwrap_or_default(),
                    depends_on: serde_json::from_str(&deps_json).unwrap_or_default(),
                    input_json: row.get(8)?,
                    output_json: row.get(9)?,
                    created_at: row.get(10)?,
                    started_at: row.get(11)?,
                    completed_at: row.get(12)?,
                    error: row.get(13)?,
                })
            })?;
            for row in rows {
                let task = row?;
                goal_tasks
                    .entry(task.goal_id.clone())
                    .or_default()
                    .push(task);
            }
        }

        // Load messages
        {
            let mut stmt = db.prepare(
                "SELECT id, goal_id, sender, content, timestamp FROM messages ORDER BY timestamp ASC"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?, // goal_id
                    GoalMessage {
                        id: row.get(0)?,
                        sender: row.get(2)?,
                        content: row.get(3)?,
                        timestamp: row.get(4)?,
                    },
                ))
            })?;
            for row in rows {
                let (goal_id, msg) = row?;
                goal_messages.entry(goal_id).or_default().push(msg);
            }
        }

        let goal_count = goals.len();
        tracing::info!("GoalEngine loaded from {db_path}: {goal_count} goals restored");

        Ok(Self {
            goals,
            goal_tasks,
            goal_messages,
            db: Some(Mutex::new(db)),
        })
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

        // Initialize conversation with a system message
        let system_msg = GoalMessage {
            id: Uuid::new_v4().to_string(),
            sender: "system".to_string(),
            content: format!("Goal submitted: {}", &goal.description),
            timestamp: now,
        };

        // Persist to SQLite
        if let Some(ref db_mutex) = self.db {
            let db = db_mutex.lock().unwrap();
            db.execute(
                "INSERT INTO goals (id, description, priority, source, status, created_at, updated_at, tags, metadata_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    goal.id, goal.description, goal.priority, goal.source,
                    goal.status, goal.created_at, goal.updated_at,
                    "[]", goal.metadata_json,
                ],
            )?;
            db.execute(
                "INSERT INTO messages (id, goal_id, sender, content, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![system_msg.id, id, system_msg.sender, system_msg.content, system_msg.timestamp],
            )?;
        }

        // Update in-memory cache
        self.goals.insert(id.clone(), goal.clone());
        self.goal_tasks.insert(id.clone(), vec![]);
        self.goal_messages.insert(id.clone(), vec![system_msg]);

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

        let completed = tasks.iter().filter(|t| t.status == "completed").count() as f64;
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

        // Persist
        if let Some(ref db_mutex) = self.db {
            let db = db_mutex.lock().unwrap();
            let _ = db.execute(
                "UPDATE goals SET status = 'cancelled', updated_at = ?1 WHERE id = ?2",
                rusqlite::params![goal.updated_at, goal_id],
            );
        }

        // Cancel all associated tasks
        if let Some(tasks) = self.goal_tasks.get_mut(goal_id) {
            for task in tasks.iter_mut() {
                if task.status != "completed" {
                    task.status = "cancelled".to_string();
                    if let Some(ref db_mutex) = self.db {
                        let db = db_mutex.lock().unwrap();
                        let _ = db.execute(
                            "UPDATE tasks SET status = 'cancelled' WHERE id = ?1",
                            rusqlite::params![task.id],
                        );
                    }
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

    /// Get tasks for a goal
    pub fn get_goal_tasks(&self, goal_id: &str) -> Vec<Task> {
        self.goal_tasks.get(goal_id).cloned().unwrap_or_default()
    }

    /// Add tasks to a goal
    pub fn add_tasks(&mut self, goal_id: &str, tasks: Vec<Task>) {
        if let Some(existing) = self.goal_tasks.get_mut(goal_id) {
            // Persist each task
            if let Some(ref db_mutex) = self.db {
                let db = db_mutex.lock().unwrap();
                for t in &tasks {
                    let tools_json = serde_json::to_string(&t.required_tools)
                        .unwrap_or_else(|_| "[]".to_string());
                    let deps_json =
                        serde_json::to_string(&t.depends_on).unwrap_or_else(|_| "[]".to_string());
                    let _ = db.execute(
                        "INSERT OR REPLACE INTO tasks (id, goal_id, description, assigned_agent, status, \
                         intelligence_level, required_tools, depends_on, input_json, output_json, \
                         created_at, started_at, completed_at, error) \
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                        rusqlite::params![
                            t.id, t.goal_id, t.description, t.assigned_agent, t.status,
                            t.intelligence_level, tools_json, deps_json, t.input_json, t.output_json,
                            t.created_at, t.started_at, t.completed_at, t.error,
                        ],
                    );
                }
            }
            existing.extend(tasks);
        }
    }

    /// Mark a task within a goal as completed
    pub fn complete_task(&mut self, goal_id: &str, task_id: &str) {
        if let Some(tasks) = self.goal_tasks.get_mut(goal_id) {
            for task in tasks.iter_mut() {
                if task.id == task_id {
                    task.status = "completed".to_string();
                    task.completed_at = chrono::Utc::now().timestamp();
                    if let Some(ref db_mutex) = self.db {
                        let db = db_mutex.lock().unwrap();
                        let _ = db.execute(
                            "UPDATE tasks SET status = 'completed', completed_at = ?1 WHERE id = ?2",
                            rusqlite::params![task.completed_at, task_id],
                        );
                    }
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
            if let Some(ref db_mutex) = self.db {
                let db = db_mutex.lock().unwrap();
                let _ = db.execute(
                    "UPDATE goals SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![status, goal.updated_at, goal_id],
                );
            }
        }
    }

    /// Set metadata on a goal (used to store preferred provider, etc.)
    pub fn set_metadata(&mut self, goal_id: &str, metadata: Vec<u8>) {
        if let Some(goal) = self.goals.get_mut(goal_id) {
            goal.metadata_json = metadata.clone();
            if let Some(ref db_mutex) = self.db {
                let db = db_mutex.lock().unwrap();
                let _ = db.execute(
                    "UPDATE goals SET metadata_json = ?1 WHERE id = ?2",
                    rusqlite::params![metadata, goal_id],
                );
            }
        }
    }

    /// Get metadata from a goal
    pub fn get_metadata(&self, goal_id: &str) -> Option<&[u8]> {
        self.goals.get(goal_id).map(|g| g.metadata_json.as_slice())
    }

    /// Add a message to a goal's conversation thread
    pub fn add_message(&mut self, goal_id: &str, sender: &str, content: &str) -> String {
        let msg_id = Uuid::new_v4().to_string();
        let msg = GoalMessage {
            id: msg_id.clone(),
            sender: sender.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        // Persist
        if let Some(ref db_mutex) = self.db {
            let db = db_mutex.lock().unwrap();
            let _ = db.execute(
                "INSERT INTO messages (id, goal_id, sender, content, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![msg.id, goal_id, msg.sender, msg.content, msg.timestamp],
            );
        }

        self.goal_messages
            .entry(goal_id.to_string())
            .or_default()
            .push(msg);
        msg_id
    }

    /// Get all messages for a goal
    pub fn get_messages(&self, goal_id: &str) -> Vec<GoalMessage> {
        self.goal_messages.get(goal_id).cloned().unwrap_or_default()
    }

    /// Get all non-terminal tasks across all goals.
    /// Used on startup to reload tasks into the TaskPlanner.
    /// Tasks that were `in_progress` at shutdown are reset to `pending`.
    pub fn get_all_resumable_tasks(&mut self) -> Vec<Task> {
        let mut tasks = Vec::new();
        for task_list in self.goal_tasks.values_mut() {
            for task in task_list.iter_mut() {
                match task.status.as_str() {
                    "pending" | "awaiting_input" => {
                        tasks.push(task.clone());
                    }
                    "in_progress" => {
                        // Was interrupted by restart — reset to pending
                        task.status = "pending".to_string();
                        if let Some(ref db_mutex) = self.db {
                            let db = db_mutex.lock().unwrap();
                            let _ = db.execute(
                                "UPDATE tasks SET status = 'pending' WHERE id = ?1",
                                rusqlite::params![task.id],
                            );
                        }
                        tasks.push(task.clone());
                    }
                    _ => {} // completed, failed, cancelled — skip
                }
            }
        }
        tasks
    }

    /// Update task status within a goal (mirrors task_planner updates)
    pub fn update_task_status(&mut self, goal_id: &str, task_id: &str, status: &str) {
        if let Some(tasks) = self.goal_tasks.get_mut(goal_id) {
            for task in tasks.iter_mut() {
                if task.id == task_id {
                    task.status = status.to_string();
                    if let Some(ref db_mutex) = self.db {
                        let db = db_mutex.lock().unwrap();
                        let _ = db.execute(
                            "UPDATE tasks SET status = ?1 WHERE id = ?2",
                            rusqlite::params![status, task_id],
                        );
                    }
                    break;
                }
            }
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

    #[tokio::test]
    async fn test_sqlite_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_goals.db");
        let db_str = db_path.to_str().unwrap();

        // Create engine, submit goal and message
        let goal_id;
        {
            let mut engine = GoalEngine::with_db(db_str).unwrap();
            goal_id = engine
                .submit_goal("Persistent goal".into(), 1, "test".into())
                .await
                .unwrap();
            engine.add_message(&goal_id, "user", "Hello from test");
            engine.update_status(&goal_id, "in_progress");
        }

        // Reopen — data should still be there
        {
            let engine = GoalEngine::with_db(db_str).unwrap();
            assert_eq!(engine.active_goal_count(), 1);
            let (goal, _tasks) = engine.get_goal_with_tasks(&goal_id).await.unwrap();
            assert_eq!(goal.description, "Persistent goal");
            assert_eq!(goal.status, "in_progress");
            let msgs = engine.get_messages(&goal_id);
            assert_eq!(msgs.len(), 2); // system + user
            assert_eq!(msgs[1].sender, "user");
            assert_eq!(msgs[1].content, "Hello from test");
        }
    }
}
