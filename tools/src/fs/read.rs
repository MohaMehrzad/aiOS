//! fs.read — Read file contents

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;

/// Read the file at `path` and return its contents as a UTF-8 string.
///
/// Input  JSON: `{ "path": "/absolute/path" }`
/// Output JSON: `{ "content": "...", "size": <u64> }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.read: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.read: missing required field 'path'"))?;

    let content =
        fs::read_to_string(path).with_context(|| format!("fs.read: failed to read {path}"))?;

    let size = content.len() as u64;

    let output = json!({
        "content": content,
        "size": size,
    });

    serde_json::to_vec(&output).context("fs.read: failed to serialise output")
}
