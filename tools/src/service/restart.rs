//! service.restart — Restart a system service (stop then start)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    name: String,
}

#[derive(Serialize)]
struct Output {
    restarted: bool,
    pid: u32,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let (restarted, pid) = if cfg!(target_os = "macos") {
        restart_launchctl(&input.name)?
    } else {
        restart_systemd(&input.name)?
    };

    let result = Output { restarted, pid };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn restart_launchctl(name: &str) -> Result<(bool, u32)> {
    // Use launchctl kickstart -k which restarts the service
    let output = Command::new("launchctl")
        .args(["kickstart", "-kp", &format!("system/{}", name)])
        .output()
        .context("Failed to execute launchctl kickstart")?;

    if output.status.success() {
        // Give the service a moment to start, then find the PID
        std::thread::sleep(std::time::Duration::from_millis(500));

        let pid = get_service_pid_launchctl(name).unwrap_or(0);
        return Ok((true, pid));
    }

    // Fallback: stop then start using plist files
    let plist_paths = [
        format!("/Library/LaunchDaemons/{}.plist", name),
        format!("/Library/LaunchAgents/{}.plist", name),
        format!(
            "{}/Library/LaunchAgents/{}.plist",
            std::env::var("HOME").unwrap_or_default(),
            name
        ),
    ];

    for path in &plist_paths {
        if std::path::Path::new(path).exists() {
            // Unload (stop)
            let _ = Command::new("launchctl")
                .args(["unload", path])
                .output();

            std::thread::sleep(std::time::Duration::from_millis(200));

            // Load (start)
            let load_output = Command::new("launchctl")
                .args(["load", "-w", path])
                .output()
                .context("Failed to execute launchctl load")?;

            if load_output.status.success() {
                let pid = get_service_pid_launchctl(name).unwrap_or(0);
                return Ok((true, pid));
            }
        }
    }

    Ok((false, 0))
}

fn get_service_pid_launchctl(name: &str) -> Option<u32> {
    let output = Command::new("launchctl").arg("list").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 && parts[2].trim() == name {
            return parts[0].trim().parse::<u32>().ok();
        }
    }

    None
}

fn restart_systemd(name: &str) -> Result<(bool, u32)> {
    let output = Command::new("systemctl")
        .args(["restart", &format!("{}.service", name)])
        .output()
        .context("Failed to execute systemctl restart")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to restart service {}: {}", name, stderr.trim());
    }

    // Get the new PID
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
