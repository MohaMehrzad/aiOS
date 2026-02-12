//! sec.check_perms — Check file permissions and ownership

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::MetadataExt;

#[derive(Deserialize)]
struct Input {
    path: String,
}

#[derive(Serialize)]
struct Output {
    owner: String,
    group: String,
    mode: String,
    writable_by_others: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let metadata = fs::metadata(&input.path)
        .with_context(|| format!("Failed to stat file: {}", input.path))?;

    let uid = metadata.uid();
    let gid = metadata.gid();
    let raw_mode = metadata.mode();

    // Get owner name from UID
    let owner = get_username(uid);

    // Get group name from GID
    let group = get_groupname(gid);

    // Format the mode as octal (just the permission bits, lower 12 bits)
    let mode = format!("{:04o}", raw_mode & 0o7777);

    // Check if the file is writable by others (o+w)
    let writable_by_others = (raw_mode & 0o002) != 0;

    let result = Output {
        owner,
        group,
        mode,
        writable_by_others,
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn get_username(uid: u32) -> String {
    // Use the id command to resolve UID to name
    let output = std::process::Command::new("id")
        .args(["-un", &uid.to_string()])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => uid.to_string(),
    }
}

fn get_groupname(gid: u32) -> String {
    // Use the id command to resolve GID to name
    // On macOS, use dscl or a stat-based approach
    let output = if cfg!(target_os = "macos") {
        std::process::Command::new("dscl")
            .args([".", "-search", "/Groups", "PrimaryGroupID", &gid.to_string()])
            .output()
    } else {
        std::process::Command::new("getent")
            .args(["group", &gid.to_string()])
            .output()
    };

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if cfg!(target_os = "macos") {
                // dscl output: "GroupName   PrimaryGroupID = (GID)"
                stdout
                    .split_whitespace()
                    .next()
                    .unwrap_or(&gid.to_string())
                    .to_string()
            } else {
                // getent output: "groupname:x:GID:"
                stdout
                    .split(':')
                    .next()
                    .unwrap_or(&gid.to_string())
                    .to_string()
            }
        }
        _ => gid.to_string(),
    }
}
