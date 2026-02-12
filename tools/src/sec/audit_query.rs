//! sec.audit_query — Query the audit log for recent tool executions

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    #[serde(default)]
    tool_name: String,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(Serialize)]
struct Output {
    entries: Vec<AuditEntry>,
}

#[derive(Serialize)]
struct AuditEntry {
    tool: String,
    agent: String,
    success: bool,
    timestamp: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = if input.is_empty() {
        Input {
            tool_name: String::new(),
            limit: default_limit(),
        }
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let limit = if input.limit == 0 {
        default_limit()
    } else {
        input.limit
    };

    // Open the audit database
    let db_path = "/var/lib/aios/ledger/audit.db";
    let conn = rusqlite::Connection::open(db_path)
        .with_context(|| format!("Failed to open audit database at {}", db_path))?;

    let entries = if input.tool_name.is_empty() {
        // Query all entries
        let mut stmt = conn
            .prepare(
                "SELECT tool_name, agent_id, success, timestamp
                 FROM audit_log
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .context("Failed to prepare query")?;

        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(AuditEntry {
                    tool: row.get::<_, String>(0)?,
                    agent: row.get::<_, String>(1)?,
                    success: row.get::<_, i32>(2)? != 0,
                    timestamp: row.get::<_, String>(3)?,
                })
            })
            .context("Failed to execute query")?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.context("Failed to read audit row")?);
        }
        entries
    } else {
        // Query filtered by tool name
        let mut stmt = conn
            .prepare(
                "SELECT tool_name, agent_id, success, timestamp
                 FROM audit_log
                 WHERE tool_name = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .context("Failed to prepare filtered query")?;

        let rows = stmt
            .query_map(rusqlite::params![input.tool_name, limit], |row| {
                Ok(AuditEntry {
                    tool: row.get::<_, String>(0)?,
                    agent: row.get::<_, String>(1)?,
                    success: row.get::<_, i32>(2)? != 0,
                    timestamp: row.get::<_, String>(3)?,
                })
            })
            .context("Failed to execute filtered query")?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.context("Failed to read audit row")?);
        }
        entries
    };

    let result = Output { entries };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
