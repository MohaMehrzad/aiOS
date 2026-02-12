//! service.start — Start a system service

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    name: String,
}

#[derive(Serialize)]
struct Output {
    started: bool,
    pid: u32,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let (started, pid) = if cfg!(target_os = "macos") {
        start_launchctl(&input.name)?
    } else {
        start_systemd(&input.name)?
    };

    let result = Output { started, pid };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn start_launchctl(name: &str) -> Result<(bool, u32)> {
    // Try to bootstrap (load + start) the service
    // First, try `launchctl kickstart` for system domain
    let output = Command::new("launchctl")
        .args(["kickstart", "-k", &format!("system/{}", name)])
        .output()
        .context("Failed to execute launchctl kickstart")?;

    if !output.status.success() {
        // Fallback: try launchctl load with common plist paths
        let plist_paths = [
            format!("/Library/LaunchDaemons/{}.plist", name),
            format!("/Library/LaunchAgents/{}.plist", name),
            format!(
                "{}/Library/LaunchAgents/{}.plist",
                std::env::var("HOME").unwrap_or_default(),
                name
            ),
            format!("/System/Library/LaunchDaemons/{}.plist", name),
        ];

        let mut loaded = false;
        for path in &plist_paths {
            if std::path::Path::new(path).exists() {
                let load_output = Command::new("launchctl")
                    .args(["load", "-w", path])
                    .output()
                    .context("Failed to execute launchctl load")?;

                if load_output.status.success() {
                    loaded = true;
                    break;
                }
            }
        }

        if !loaded {
            return Ok((false, 0));
        }
    }

    // Get the PID of the now-running service
    let pid = get_launchctl_pid(name).unwrap_or(0);
    Ok((true, pid))
}

fn get_launchctl_pid(name: &str) -> Option<u32> {
    let output = Command::new("launchctl")
        .args(["list", name])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // launchctl list <label> outputs key-value pairs
    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("\"PID\"") || line.contains("PID") {
            // Parse PID from the output
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(last) = parts.last() {
                let cleaned = last.trim_end_matches(';').trim_matches('"');
                if let Ok(pid) = cleaned.parse::<u32>() {
                    return Some(pid);
                }
            }
        }
    }

    // Alternatively, parse the first column of `launchctl list` output
    let list_output = Command::new("launchctl").arg("list").output().ok()?;
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 && parts[2].trim() == name {
            return parts[0].trim().parse::<u32>().ok();
        }
    }

    None
}

fn start_systemd(name: &str) -> Result<(bool, u32)> {
    let output = Command::new("systemctl")
        .args(["start", &format!("{}.service", name)])
        .output()
        .context("Failed to execute systemctl start")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to start service {}: {}", name, stderr.trim());
    }

    // Get the PID
    let pid_output = Command::new("systemctl")
        .args(["show", "-p", "MainPID", &format!("{}.service", name)])
        .output()
        .context("Failed to get service PID")?;

    let stdout = String::from_utf8_lossy(&pid_output.stdout);
    let pid = stdout
        .trim()
        .strip_prefix("MainPID=")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    Ok((true, pid))
}
