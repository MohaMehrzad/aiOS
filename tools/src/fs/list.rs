//! fs.list — List directory contents

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::time::UNIX_EPOCH;

/// Return the immediate children of the directory at `path`.
///
/// Each entry includes its name, type ("file", "dir", or "symlink"), size in
/// bytes, and last-modified timestamp as an ISO-8601 string.
///
/// Input  JSON: `{ "path": "/absolute/dir" }`
/// Output JSON: `{ "entries": [{ "name": "...", "type": "...", "size": u64, "modified": "..." }] }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.list: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.list: missing required field 'path'"))?;

    let read_dir =
        fs::read_dir(path).with_context(|| format!("fs.list: cannot read directory {path}"))?;

    let mut entries = Vec::new();

    for entry_result in read_dir {
        let entry =
            entry_result.with_context(|| format!("fs.list: error iterating directory {path}"))?;

        let name = entry.file_name().to_string_lossy().to_string();

        // Use the DirEntry file_type (does not follow symlinks) to detect symlinks,
        // then use symlink_metadata for size so we report the link itself, not the target.
        let ft = entry.file_type().with_context(|| {
            format!(
                "fs.list: cannot get file type for {}",
                entry.path().display()
            )
        })?;

        let metadata = fs::symlink_metadata(entry.path())
            .with_context(|| format!("fs.list: cannot stat {}", entry.path().display()))?;

        let file_type = if ft.is_symlink() {
            "symlink"
        } else if ft.is_dir() {
            "dir"
        } else {
            "file"
        };

        let size = metadata.len();

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| {
                chrono::DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
                    .unwrap_or_default()
                    .to_rfc3339()
            })
            .unwrap_or_default();

        entries.push(json!({
            "name": name,
            "type": file_type,
            "size": size,
            "modified": modified,
        }));
    }

    let output = json!({ "entries": entries });
    serde_json::to_vec(&output).context("fs.list: failed to serialise output")
}
