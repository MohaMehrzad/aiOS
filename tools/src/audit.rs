//! Audit logging — hash-chained ledger of all tool executions

use anyhow::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tracing::info;

/// Hash-chained audit ledger stored in SQLite
pub struct AuditLog {
    conn: Connection,
    last_hash: String,
}

impl AuditLog {
    pub fn new(db_path: &str) -> Result<Self> {
        // Create parent directory if needed
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                execution_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                success INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                prev_hash TEXT NOT NULL,
                hash TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_audit_tool ON audit_log(tool_name);
            CREATE INDEX IF NOT EXISTS idx_audit_agent ON audit_log(agent_id);
            CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(timestamp);",
        )?;

        // Load last hash for chain continuity
        let last_hash = conn
            .query_row(
                "SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "genesis".to_string());

        Ok(Self { conn, last_hash })
    }

    /// Record an audit entry with hash chaining
    pub fn record(
        &mut self,
        execution_id: &str,
        tool_name: &str,
        agent_id: &str,
        task_id: &str,
        reason: &str,
        success: bool,
        duration_ms: i64,
    ) {
        let timestamp = chrono::Utc::now().to_rfc3339();

        // Compute hash: SHA256(prev_hash + execution_id + tool_name + agent_id + timestamp)
        let mut hasher = Sha256::new();
        hasher.update(&self.last_hash);
        hasher.update(execution_id);
        hasher.update(tool_name);
        hasher.update(agent_id);
        hasher.update(&timestamp);
        let hash = format!("{:x}", hasher.finalize());

        let result = self.conn.execute(
            "INSERT INTO audit_log (execution_id, tool_name, agent_id, task_id, reason, success, duration_ms, timestamp, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                execution_id,
                tool_name,
                agent_id,
                task_id,
                reason,
                success as i32,
                duration_ms,
                timestamp,
                &self.last_hash,
                &hash,
            ],
        );

        match result {
            Ok(_) => {
                self.last_hash = hash;
                info!(
                    "Audit: tool={tool_name} agent={agent_id} success={success} duration={duration_ms}ms"
                );
            }
            Err(e) => {
                tracing::error!("Failed to write audit log: {e}");
            }
        }
    }

    /// Verify the audit chain integrity
    pub fn verify_chain(&self) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT execution_id, tool_name, agent_id, timestamp, prev_hash, hash FROM audit_log ORDER BY id ASC",
        )?;

        let mut expected_prev = "genesis".to_string();
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        for row in rows {
            let (exec_id, tool_name, agent_id, timestamp, prev_hash, stored_hash) = row?;

            // Verify prev_hash matches what we expect
            if prev_hash != expected_prev {
                return Ok(false);
            }

            // Recompute hash
            let mut hasher = Sha256::new();
            hasher.update(&prev_hash);
            hasher.update(&exec_id);
            hasher.update(&tool_name);
            hasher.update(&agent_id);
            hasher.update(&timestamp);
            let computed = format!("{:x}", hasher.finalize());

            if computed != stored_hash {
                return Ok(false);
            }

            expected_prev = stored_hash;
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_audit_log() {
        let tmp = NamedTempFile::new().unwrap();
        let mut log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();

        log.record("exec-1", "fs.read", "agent-1", "task-1", "test", true, 50);
        log.record("exec-2", "fs.write", "agent-1", "task-1", "test", true, 100);

        assert!(log.verify_chain().unwrap());
    }

    #[test]
    fn test_audit_log_empty_chain() {
        let tmp = NamedTempFile::new().unwrap();
        let log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();
        // Empty chain should verify as valid
        assert!(log.verify_chain().unwrap());
    }

    #[test]
    fn test_audit_log_single_entry() {
        let tmp = NamedTempFile::new().unwrap();
        let mut log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();

        log.record(
            "exec-1",
            "fs.read",
            "agent-1",
            "task-1",
            "test read",
            true,
            25,
        );
        assert!(log.verify_chain().unwrap());
    }

    #[test]
    fn test_audit_log_many_entries() {
        let tmp = NamedTempFile::new().unwrap();
        let mut log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();

        for i in 0..100 {
            log.record(
                &format!("exec-{i}"),
                &format!("tool-{}", i % 5),
                &format!("agent-{}", i % 3),
                &format!("task-{}", i % 10),
                "bulk test",
                i % 7 != 0, // some failures
                50 + (i as i64) * 10,
            );
        }

        assert!(log.verify_chain().unwrap());
    }

    #[test]
    fn test_audit_log_with_failure() {
        let tmp = NamedTempFile::new().unwrap();
        let mut log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();

        log.record("exec-1", "fs.read", "agent-1", "task-1", "test", true, 50);
        log.record(
            "exec-2",
            "fs.write",
            "agent-1",
            "task-1",
            "write failed",
            false,
            200,
        );
        log.record("exec-3", "fs.read", "agent-2", "task-2", "retry", true, 75);

        assert!(log.verify_chain().unwrap());
    }

    #[test]
    fn test_audit_log_chain_persistence() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        // Write some entries
        {
            let mut log = AuditLog::new(&path).unwrap();
            log.record("exec-1", "fs.read", "agent-1", "task-1", "test", true, 50);
            log.record("exec-2", "fs.write", "agent-1", "task-1", "test", true, 100);
        }

        // Reopen and continue the chain
        {
            let mut log = AuditLog::new(&path).unwrap();
            log.record(
                "exec-3",
                "fs.delete",
                "agent-2",
                "task-2",
                "cleanup",
                true,
                30,
            );
            assert!(log.verify_chain().unwrap());
        }
    }

    #[test]
    fn test_audit_log_genesis_hash() {
        let tmp = NamedTempFile::new().unwrap();
        let log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();
        // Before any entries, last_hash should be "genesis"
        assert_eq!(log.last_hash, "genesis");
    }

    #[test]
    fn test_audit_log_hash_changes_after_record() {
        let tmp = NamedTempFile::new().unwrap();
        let mut log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();

        let hash_before = log.last_hash.clone();
        log.record("exec-1", "fs.read", "agent-1", "task-1", "test", true, 50);
        let hash_after = log.last_hash.clone();

        assert_ne!(hash_before, hash_after);
        assert_ne!(hash_after, "genesis");
    }

    #[test]
    fn test_audit_log_different_agents() {
        let tmp = NamedTempFile::new().unwrap();
        let mut log = AuditLog::new(tmp.path().to_str().unwrap()).unwrap();

        log.record(
            "exec-1",
            "fs.read",
            "sys-agent",
            "task-1",
            "system read",
            true,
            50,
        );
        log.record(
            "exec-2",
            "net.ping",
            "net-agent",
            "task-2",
            "network check",
            true,
            100,
        );
        log.record(
            "exec-3",
            "sec.audit",
            "sec-agent",
            "task-3",
            "security scan",
            true,
            500,
        );

        assert!(log.verify_chain().unwrap());
    }
}
