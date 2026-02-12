//! fs.chown — Change file ownership

use anyhow::{Context, Result};
use nix::unistd::{chown, Gid, Uid};
use serde_json::json;
use std::path::Path;

/// Change the owner and group of the entry at `path`.
///
/// Input  JSON: `{ "path": "/absolute/path", "uid": 1000, "gid": 1000 }`
/// Output JSON: `{ "changed": true }`
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let v: serde_json::Value =
        serde_json::from_slice(input).context("fs.chown: invalid JSON input")?;

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("fs.chown: missing required field 'path'"))?;

    let uid = v
        .get("uid")
        .and_then(|u| u.as_u64())
        .ok_or_else(|| anyhow::anyhow!("fs.chown: missing required field 'uid'"))?
        as u32;

    let gid = v
        .get("gid")
        .and_then(|g| g.as_u64())
        .ok_or_else(|| anyhow::anyhow!("fs.chown: missing required field 'gid'"))?
        as u32;

    chown(
        Path::new(path),
        Some(Uid::from_raw(uid)),
        Some(Gid::from_raw(gid)),
    )
    .with_context(|| format!("fs.chown: failed to chown {path} to uid={uid} gid={gid}"))?;

    let output = json!({ "changed": true });
    serde_json::to_vec(&output).context("fs.chown: failed to serialise output")
}
