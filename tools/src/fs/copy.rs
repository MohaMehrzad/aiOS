//! fs.copy — Copy a file to a new location

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::Path;

/// Copy the file at `source` to `destination`.
///
/// Parent directories for the destination are created automatically.
/// If `source` is a directory the copy is recursive.
///
/// Input  JSON: `{ "source": "/abs/src", "destination": "/abs/dst" }`
/// Output JSON: `{ "copied": true }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.copy: invalid JSON input")?;

    let source = v
        .get("source")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.copy: missing required field 'source'"))?;

    let destination = v
        .get("destination")
        .and_then(|d| d.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.copy: missing required field 'destination'"))?;

    let src = Path::new(source);
    if !src.exists() {
        anyhow::bail!("fs.copy: source does not exist: {source}");
    }

    // Create parent directories for destination
    if let Some(parent) = Path::new(destination).parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("fs.copy: cannot create parent dirs for {destination}"))?;
        }
    }

    if src.is_dir() {
        copy_dir_recursive(src, Path::new(destination)).with_context(|| {
            format!("fs.copy: failed to copy directory {source} -> {destination}")
        })?;
    } else {
        fs::copy(source, destination)
            .with_context(|| format!("fs.copy: failed to copy {source} -> {destination}"))?;
    }

    let output = json!({ "copied": true });
    serde_json::to_vec(&output).context("fs.copy: failed to serialise output")
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry_result in fs::read_dir(src)? {
        let entry = entry_result?;
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            fs::copy(&entry_path, &dest_path)?;
        }
    }

    Ok(())
}
