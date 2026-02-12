//! fs.write — Write content to a file (with optional backup)

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::Path;

/// Write `content` to the file at `path`.
///
/// If the file already exists a backup is written to `<path>.bak` before
/// overwriting so the caller can roll back manually if the backup manager is
/// not involved.
///
/// Input  JSON: `{ "path": "/absolute/path", "content": "..." }`
/// Output JSON: `{ "bytes_written": <u64> }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.write: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.write: missing required field 'path'"))?;

    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.write: missing required field 'content'"))?;

    // Create parent directories if they don't exist
    if let Some(parent) = Path::new(path).parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("fs.write: cannot create parent dirs for {path}"))?;
        }
    }

    // Back up existing file
    if Path::new(path).exists() {
        let backup_path = format!("{path}.bak");
        fs::copy(path, &backup_path)
            .with_context(|| format!("fs.write: failed to create backup at {backup_path}"))?;
    }

    let bytes = content.as_bytes();
    fs::write(path, bytes).with_context(|| format!("fs.write: failed to write {path}"))?;

    let output = json!({
        "bytes_written": bytes.len() as u64,
    });

    serde_json::to_vec(&output).context("fs.write: failed to serialise output")
}
