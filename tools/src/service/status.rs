//! service.status — Get detailed status of a system service

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    name: String,
}

#[derive(Serialize)]
struct Output {
    name: String,
    status: String,
    pid: Option<u32>,
    uptime: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let result = if cfg!(target_os = "macos") {
        status_launchctl(&input.name)?
    } else {
        status_systemd(&input.name)?
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn status_launchctl(name: &str) -> Result<Output> {
    // Get service info from launchctl list
    let output = Command::new("launchctl")
        .arg("list")
        .output()
        .context("Failed to execute launchctl list")?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 && parts[2].trim() == name {
            let pid = parts[0].trim().parse::<u32>().ok();
            let exit_status = parts[1].trim();

            let status = if pid.is_some() {
                "running".to_string()
            } else if exit_status == "0" {
                "stopped".to_string()
            } else {
                format!("exited({})", exit_status)
            };

            // Calculate uptime if the process is running
            let uptime = if let Some(pid_val) = pid {
                get_process_uptime(pid_val)
            } else {
                "N/A".to_string()
            };

            return Ok(Output {
                name: name.to_string(),
                status,
                pid,
                uptime,
            });
        }
    }

    // Service not found in launchctl list
    Ok(Output {
        name: name.to_string(),
        status: "not_found".to_string(),
        pid: None,
        uptime: "N/A".to_string(),
    })
}

fn get_process_uptime(pid: u32) -> String {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "etime="])
        .output();

    match output {
        Ok(out) => {
            let etime = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if etime.is_empty() {
                "unknown".to_string()
            } else {
                etime
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

fn status_systemd(name: &str) -> Result<Output> {
    let service_name = format!("{}.service", name);

    // Get active state
    let active_output = Command::new("systemctl")
        .args(["show", "-p", "ActiveState", &service_name])
        .output()
        .context("Failed to execute systemctl show")?;

    let active_stdout = String::from_utf8_lossy(&active_output.stdout);
    let status = active_stdout
        .trim()
        .strip_prefix("ActiveState=")
        .unwrap_or("unknown")
        .to_string();

    // Get PID
    let pid_output = Command::new("systemctl")
        .args(["show", "-p", "MainPID", &service_name])
        .output()
        .context("Failed to get MainPID")?;

    let pid_stdout = String::from_utf8_lossy(&pid_output.stdout);
    let pid = pid_stdout
        .trim()
        .strip_prefix("MainPID=")
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&p| p != 0);

    // Get uptime from ActiveEnterTimestamp
    let time_output = Command::new("systemctl")
        .args(["show", "-p", "ActiveEnterTimestamp", &service_name])
        .output()
        .context("Failed to get ActiveEnterTimestamp")?;

    let time_stdout = String::from_utf8_lossy(&time_output.stdout);
    let uptime = time_stdout
        .trim()
        .strip_prefix("ActiveEnterTimestamp=")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "N/A".to_string());

    Ok(Output {
        name: name.to_string(),
        status,
        pid,
        uptime,
    })
}
