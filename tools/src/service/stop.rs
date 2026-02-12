//! service.stop — Stop a system service

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    name: String,
}

#[derive(Serialize)]
struct Output {
    stopped: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let stopped = if cfg!(target_os = "macos") {
        stop_launchctl(&input.name)?
    } else {
        stop_systemd(&input.name)?
    };

    let result = Output { stopped };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn stop_launchctl(name: &str) -> Result<bool> {
    // Try launchctl bootout for system domain
    let output = Command::new("launchctl")
        .args(["bootout", &format!("system/{}", name)])
        .output();

    if let Ok(ref out) = output {
        if out.status.success() {
            return Ok(true);
        }
    }

    // Fallback: try launchctl unload with common plist paths
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

    for path in &plist_paths {
        if std::path::Path::new(path).exists() {
            let unload_output = Command::new("launchctl")
                .args(["unload", path])
                .output()
                .context("Failed to execute launchctl unload")?;

            if unload_output.status.success() {
                return Ok(true);
            }
        }
    }

    // If we can find the PID, try to stop via kill as a last resort
    let list_output = Command::new("launchctl").arg("list").output().ok();
    if let Some(out) = list_output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 && parts[2].trim() == name {
                if let Ok(pid) = parts[0].trim().parse::<i32>() {
                    let kill_result = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGTERM,
                    );
                    return Ok(kill_result.is_ok());
                }
            }
        }
    }

    Ok(false)
}

fn stop_systemd(name: &str) -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["stop", &format!("{}.service", name)])
        .output()
        .context("Failed to execute systemctl stop")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stop service {}: {}", name, stderr.trim());
    }

    Ok(true)
}
