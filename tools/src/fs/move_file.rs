//! fs.move — Move or rename a file / directory

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::Path;

/// Move (or rename) `source` to `destination`.
///
/// This first attempts `std::fs::rename` which is atomic on the same
/// filesystem. If rename fails (e.g. cross-device move) it falls back to
/// copy-then-delete.
///
/// Input  JSON: `{ "source": "/abs/src", "destination": "/abs/dst" }`
/// Output JSON: `{ "moved": true }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.move: invalid JSON input")?;

    let source = v
        .get("source")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.move: missing required field 'source'"))?;

    let destination = v
        .get("destination")
        .and_then(|d| d.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.move: missing required field 'destination'"))?;

    if !Path::new(source).exists() {
        anyhow::bail!("fs.move: source does not exist: {source}");
    }

    // Create parent directories for destination if needed
    if let Some(parent) = Path::new(destination).parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).with_context(|| {
                format!("fs.move: cannot create parent dirs for destination {destination}")
            })?;
        }
    }

    // Try atomic rename first
    match fs::rename(source, destination) {
        Ok(()) => {}
        Err(_rename_err) => {
            // Fallback: copy then delete (handles cross-device moves)
            if Path::new(source).is_dir() {
                copy_dir_recursive(Path::new(source), Path::new(destination)).with_context(
                    || format!("fs.move: failed to copy directory {source} -> {destination}"),
                )?;
                fs::remove_dir_all(source).with_context(|| {
                    format!("fs.move: copied but failed to remove original directory {source}")
                })?;
            } else {
                fs::copy(source, destination).with_context(|| {
                    format!("fs.move: failed to copy {source} -> {destination}")
                })?;
                fs::remove_file(source).with_context(|| {
                    format!("fs.move: copied but failed to remove original file {source}")
                })?;
            }
        }
    }

    let output = json!({ "moved": true });
    serde_json::to_vec(&output).context("fs.move: failed to serialise output")
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
