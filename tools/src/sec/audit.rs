//! sec.audit — Query the audit ledger with filters

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct AuditInput {
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    since: String,
    #[serde(default)]
    until: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    100
}

#[derive(Serialize)]
struct AuditEntry {
    id: i64,
    timestamp: String,
    agent_id: String,
    tool_name: String,
    action: String,
    success: bool,
}

#[derive(Serialize)]
struct AuditOutput {
    entries: Vec<AuditEntry>,
    total: usize,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: AuditInput = serde_json::from_slice(input).context("Invalid sec.audit input")?;

    let db_path = "/var/lib/aios/data/audit.db";
    let conn = rusqlite::Connection::open(db_path).context("Failed to open audit database")?;

    let mut sql = String::from(
        "SELECT id, timestamp, agent_id, tool_name, action, success FROM audit_log WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !req.agent_id.is_empty() {
        sql.push_str(" AND agent_id = ?");
        params.push(Box::new(req.agent_id.clone()));
    }
    if !req.tool_name.is_empty() {
        sql.push_str(" AND tool_name = ?");
        params.push(Box::new(req.tool_name.clone()));
    }
    if !req.since.is_empty() {
        sql.push_str(" AND timestamp >= ?");
        params.push(Box::new(req.since.clone()));
    }
    if !req.until.is_empty() {
        sql.push_str(" AND timestamp <= ?");
        params.push(Box::new(req.until.clone()));
    }

    sql.push_str(" ORDER BY id DESC LIMIT ?");
    params.push(Box::new(req.limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .context("Failed to prepare audit query")?;
    let entries: Vec<AuditEntry> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(AuditEntry {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                agent_id: row.get(2)?,
                tool_name: row.get(3)?,
                action: row.get(4)?,
                success: row.get::<_, i32>(5)? != 0,
            })
        })
        .context("Failed to execute audit query")?
        .filter_map(|r| r.ok())
        .collect();

    let total = entries.len();
    let output = AuditOutput { entries, total };
    serde_json::to_vec(&output).context("Failed to serialize output")
}
