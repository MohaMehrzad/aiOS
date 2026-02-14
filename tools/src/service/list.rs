//! service.list — List system services

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    services: Vec<ServiceEntry>,
}

#[derive(Serialize)]
struct ServiceEntry {
    name: String,
    status: String,
    pid: Option<u32>,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let mut services = Vec::new();

    // On macOS, use launchctl list to enumerate services
    // Output format: PID\tStatus\tLabel
    if cfg!(target_os = "macos") {
        let output = Command::new("launchctl")
            .arg("list")
            .output()
            .context("Failed to execute launchctl list")?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }

            let pid = parts[0].trim().parse::<u32>().ok();
            let exit_status = parts[1].trim();
            let label = parts[2].trim().to_string();

            let status = if pid.is_some() {
                "running".to_string()
            } else if exit_status == "0" {
                "stopped".to_string()
            } else {
                format!("exited({})", exit_status)
            };

            services.push(ServiceEntry {
                name: label,
                status,
                pid,
            });
        }
    } else {
        // On Linux, use systemctl
        let output = Command::new("systemctl")
            .args([
                "list-units",
                "--type=service",
                "--all",
                "--no-pager",
                "--no-legend",
            ])
            .output()
            .context("Failed to execute systemctl")?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }

            let name = parts[0]
                .strip_suffix(".service")
                .unwrap_or(parts[0])
                .to_string();
            let active = parts[2].to_string();
            let sub = parts[3];

            let pid = if sub == "running" {
                // Try to get PID from systemctl show
                get_systemd_pid(&name)
            } else {
                None
            };

            services.push(ServiceEntry {
                name,
                status: active,
                pid,
            });
        }
    }

    let result = Output { services };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

#[cfg(not(target_os = "macos"))]
fn get_systemd_pid(name: &str) -> Option<u32> {
    let output = Command::new("systemctl")
        .args(["show", "-p", "MainPID", &format!("{}.service", name)])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pid_str = stdout.trim().strip_prefix("MainPID=")?;
    let pid = pid_str.parse::<u32>().ok()?;
    if pid == 0 {
        None
    } else {
        Some(pid)
    }
}

#[cfg(target_os = "macos")]
fn get_systemd_pid(_name: &str) -> Option<u32> {
    None
}
