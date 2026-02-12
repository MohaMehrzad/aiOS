//! fs.symlink — Create a symbolic link

use anyhow::{Context, Result};
use serde_json::json;
use std::os::unix::fs::symlink as unix_symlink;
use std::path::Path;

/// Create a symbolic link at `link` that points to `target`.
///
/// Input  JSON: `{ "target": "/abs/original", "link": "/abs/link_name" }`
/// Output JSON: `{ "created": true }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.symlink: invalid JSON input")?;

    let target = v
        .get("target")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.symlink: missing required field 'target'"))?;

    let link = v
        .get("link")
        .and_then(|l| l.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.symlink: missing required field 'link'"))?;

    // Create parent directories for the link if they don't exist
    if let Some(parent) = Path::new(link).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("fs.symlink: cannot create parent dirs for {link}")
            })?;
        }
    }

    unix_symlink(target, link)
        .with_context(|| format!("fs.symlink: failed to create symlink {link} -> {target}"))?;

    let output = json!({ "created": true });
    serde_json::to_vec(&output).context("fs.symlink: failed to serialise output")
}
