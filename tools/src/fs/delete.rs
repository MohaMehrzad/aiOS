//! fs.delete — Delete a file or directory

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::Path;

/// Delete the entry at `path`.
///
/// When `recursive` is true and the path is a directory the entire subtree is
/// removed (`rm -rf` semantics). When `recursive` is false and the path is a
/// non-empty directory the call fails.
///
/// Input  JSON: `{ "path": "/absolute/path", "recursive": bool }`
/// Output JSON: `{ "deleted": true }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.delete: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.delete: missing required field 'path'"))?;

    let recursive = v
        .get("recursive")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);

    let p = Path::new(path);

    if !p.exists() {
        anyhow::bail!("fs.delete: path does not exist: {path}");
    }

    if p.is_dir() {
        if recursive {
            fs::remove_dir_all(path)
                .with_context(|| format!("fs.delete: failed to recursively remove {path}"))?;
        } else {
            fs::remove_dir(path)
                .with_context(|| format!("fs.delete: failed to remove directory {path} (is it empty?)"))?;
        }
    } else {
        fs::remove_file(path)
            .with_context(|| format!("fs.delete: failed to remove file {path}"))?;
    }

    let output = json!({ "deleted": true });
    serde_json::to_vec(&output).context("fs.delete: failed to serialise output")
}
