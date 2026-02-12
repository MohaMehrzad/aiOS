//! fs.stat — Return file / directory metadata

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::UNIX_EPOCH;

/// Return metadata for the entry at `path`.
///
/// Input  JSON: `{ "path": "/absolute/path" }`
/// Output JSON:
/// ```json
/// {
///     "size": u64,
///     "permissions": "0644",
///     "modified": "2024-01-01T00:00:00+00:00",
///     "accessed": "2024-01-01T00:00:00+00:00",
///     "is_dir": bool,
///     "is_file": bool,
///     "is_symlink": bool
/// }
/// ```
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.stat: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.stat: missing required field 'path'"))?;

    // Use symlink_metadata so we can detect symlinks without following them
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("fs.stat: cannot stat {path}"))?;

    let size = metadata.len();
    let mode = metadata.mode();
    let permissions = format!("{:04o}", mode & 0o7777);

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

    let accessed = metadata
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| {
            chrono::DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
                .unwrap_or_default()
                .to_rfc3339()
        })
        .unwrap_or_default();

    let is_symlink = metadata.file_type().is_symlink();
    let is_dir = metadata.is_dir();
    let is_file = metadata.is_file();

    let output = json!({
        "size": size,
        "permissions": permissions,
        "modified": modified,
        "accessed": accessed,
        "is_dir": is_dir,
        "is_file": is_file,
        "is_symlink": is_symlink,
    });

    serde_json::to_vec(&output).context("fs.stat: failed to serialise output")
}
