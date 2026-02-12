//! fs.disk_usage — Report disk usage for a filesystem

use anyhow::{Context, Result};
use nix::sys::statvfs::statvfs;
use serde_json::json;

/// Return disk-usage statistics for the filesystem that contains `path`.
///
/// Uses `nix::sys::statvfs` which wraps the POSIX `statvfs` syscall.
///
/// Input  JSON: `{ "path": "/some/path" }`
/// Output JSON:
/// ```json
/// {
///     "total_bytes": u64,
///     "used_bytes": u64,
///     "available_bytes": u64,
///     "usage_percent": f64
/// }
/// ```
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.disk_usage: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.disk_usage: missing required field 'path'"))?;

    let stat = statvfs(path)
        .with_context(|| format!("fs.disk_usage: statvfs failed for {path}"))?;

    let block_size = stat.fragment_size() as u64;
    let total_bytes = stat.blocks() as u64 * block_size;
    let available_bytes = stat.blocks_available() as u64 * block_size;
    let used_bytes = total_bytes.saturating_sub(available_bytes);

    let usage_percent = if total_bytes > 0 {
        (used_bytes as f64 / total_bytes as f64) * 100.0
    } else {
        0.0
    };

    // Round to two decimal places
    let usage_percent = (usage_percent * 100.0).round() / 100.0;

    let output = json!({
        "total_bytes": total_bytes,
        "used_bytes": used_bytes,
        "available_bytes": available_bytes,
        "usage_percent": usage_percent,
    });

    serde_json::to_vec(&output).context("fs.disk_usage: failed to serialise output")
}
