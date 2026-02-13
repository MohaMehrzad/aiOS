//! sec.revoke — Revoke capabilities from an agent

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct RevokeInput {
    agent_id: String,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    revoke_all: bool,
}

#[derive(Serialize)]
struct RevokeOutput {
    success: bool,
    agent_id: String,
    revoked_count: usize,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: RevokeInput = serde_json::from_slice(input).context("Invalid sec.revoke input")?;

    let db_path = "/var/lib/aios/data/capabilities.db";
    let conn =
        rusqlite::Connection::open(db_path).context("Failed to open capabilities database")?;

    let revoked = if req.revoke_all {
        conn.execute(
            "UPDATE capability_grants SET revoked = 1 WHERE agent_id = ?1 AND revoked = 0",
            rusqlite::params![req.agent_id],
        )
        .context("Failed to revoke all capabilities")?
    } else {
        let mut total = 0;
        for cap in &req.capabilities {
            total += conn
                .execute(
                    "UPDATE capability_grants SET revoked = 1 WHERE agent_id = ?1 AND capability = ?2 AND revoked = 0",
                    rusqlite::params![req.agent_id, cap],
                )
                .context("Failed to revoke capability")?;
        }
        total
    };

    let output = RevokeOutput {
        success: true,
        agent_id: req.agent_id,
        revoked_count: revoked,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}
