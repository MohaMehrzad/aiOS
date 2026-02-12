//! fs.chmod — Change file permissions

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// Set the permission bits for the entry at `path` to the octal value given
/// in `mode` (e.g. "0755", "644").
///
/// Input  JSON: `{ "path": "/absolute/path", "mode": "0755" }`
/// Output JSON: `{ "changed": true }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.chmod: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.chmod: missing required field 'path'"))?;

    let mode_str = v
        .get("mode")
        .and_then(|m| m.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.chmod: missing required field 'mode'"))?;

    // Parse the octal string. Accept optional leading "0" or "0o" prefix.
    let stripped = mode_str
        .strip_prefix("0o")
        .or_else(|| mode_str.strip_prefix("0O"))
        .unwrap_or(mode_str);

    let mode = u32::from_str_radix(stripped, 8)
        .with_context(|| format!("fs.chmod: invalid octal mode string: {mode_str}"))?;

    let metadata =
        fs::metadata(path).with_context(|| format!("fs.chmod: cannot stat {path}"))?;

    let mut permissions = metadata.permissions();
    permissions.set_mode(mode);

    fs::set_permissions(path, permissions)
        .with_context(|| format!("fs.chmod: failed to set permissions on {path}"))?;

    let output = json!({ "changed": true });
    serde_json::to_vec(&output).context("fs.chmod: failed to serialise output")
}
