//! fs.mkdir — Create a directory

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;

/// Create the directory at `path`.
///
/// When `recursive` is true all missing parent directories are created as
/// well (`mkdir -p` semantics). If the directory already exists the call
/// succeeds without error regardless of the `recursive` flag.
///
/// Input  JSON: `{ "path": "/absolute/path", "recursive": bool }`
/// Output JSON: `{ "created": true }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.mkdir: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.mkdir: missing required field 'path'"))?;

    let recursive = v
        .get("recursive")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);

    if recursive {
        fs::create_dir_all(path)
            .with_context(|| format!("fs.mkdir: failed to create directory tree {path}"))?;
    } else {
        fs::create_dir(path)
            .with_context(|| format!("fs.mkdir: failed to create directory {path}"))?;
    }

    let output = json!({ "created": true });
    serde_json::to_vec(&output).context("fs.mkdir: failed to serialise output")
}
