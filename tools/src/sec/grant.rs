//! sec.grant — Grant capabilities to an agent
//!
//! Writes capability grants to a SQLite database.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct GrantInput {
    agent_id: String,
    capabilities: Vec<String>,
    #[serde(default)]
    reason: String,
    #[serde(default = "default_duration")]
    duration_hours: i64,
}

fn default_duration() -> i64 {
    24
}

#[derive(Serialize)]
struct GrantOutput {
    success: bool,
    agent_id: String,
    granted: Vec<String>,
    expires_at: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: GrantInput = serde_json::from_slice(input).context("Invalid sec.grant input")?;

    let db_path = "/var/lib/aios/data/capabilities.db";
    std::fs::create_dir_all("/var/lib/aios/data").ok();
    let conn =
        rusqlite::Connection::open(db_path).context("Failed to open capabilities database")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS capability_grants (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            agent_id TEXT NOT NULL,
            capability TEXT NOT NULL,
            reason TEXT,
            granted_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            revoked INTEGER DEFAULT 0
        )",
    )
    .context("Failed to create capability_grants table")?;

    let now = chrono::Utc::now();
    let expires = now + chrono::Duration::hours(req.duration_hours);
    let now_str = now.to_rfc3339();
    let expires_str = expires.to_rfc3339();

    for cap in &req.capabilities {
        conn.execute(
            "INSERT INTO capability_grants (agent_id, capability, reason, granted_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![req.agent_id, cap, req.reason, now_str, expires_str],
        )
        .context("Failed to insert capability grant")?;
    }

    let output = GrantOutput {
        success: true,
        agent_id: req.agent_id,
        granted: req.capabilities,
        expires_at: expires_str,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}
