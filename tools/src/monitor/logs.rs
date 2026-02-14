//! monitor.logs — Read system log entries

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    #[serde(default = "default_lines")]
    lines: u32,
    #[serde(default)]
    service: String,
}

fn default_lines() -> u32 {
    100
}

#[derive(Serialize)]
struct Output {
    entries: Vec<String>,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = if input.is_empty() {
        Input {
            lines: default_lines(),
            service: String::new(),
        }
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let lines = if input.lines == 0 {
        default_lines()
    } else {
        input.lines
    };

    let entries = if cfg!(target_os = "macos") {
        read_logs_macos(lines, &input.service)?
    } else {
        read_logs_linux(lines, &input.service)?
    };

    let result = Output { entries };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn read_logs_macos(lines: u32, service: &str) -> Result<Vec<String>> {
    // Use the `log` command on macOS with --last to get recent entries
    let mut cmd = Command::new("log");
    cmd.args(["show", "--last", "1h", "--style", "compact"]);

    if !service.is_empty() {
        cmd.args(["--predicate", &format!("subsystem == '{}'", service)]);
    }

    let output = cmd.output().context("Failed to execute log show command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<String> = stdout
        .lines()
        .rev() // Most recent first
        .take(lines as usize)
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .rev() // Restore chronological order
        .collect();

    Ok(entries)
}

fn read_logs_linux(lines: u32, service: &str) -> Result<Vec<String>> {
    // Use journalctl on Linux
    let mut cmd = Command::new("journalctl");
    cmd.args(["-n", &lines.to_string(), "--no-pager", "-o", "short-iso"]);

    if !service.is_empty() {
        cmd.args(["-u", &format!("{}.service", service)]);
    }

    let output = cmd
        .output()
        .context("Failed to execute journalctl command")?;

    if !output.status.success() {
        // Fallback: try reading /var/log/syslog or /var/log/messages
        return read_logs_file(lines, service);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<String> = stdout.lines().map(|l| l.to_string()).collect();

    Ok(entries)
}

fn read_logs_file(lines: u32, service: &str) -> Result<Vec<String>> {
    let log_paths = ["/var/log/syslog", "/var/log/messages"];

    for path in &log_paths {
        if std::path::Path::new(path).exists() {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path))?;

            let all_lines: Vec<&str> = content.lines().collect();

            let filtered: Vec<String> = if service.is_empty() {
                all_lines
                    .iter()
                    .rev()
                    .take(lines as usize)
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            } else {
                all_lines
                    .iter()
                    .filter(|l| l.contains(service))
                    .rev()
                    .take(lines as usize)
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            };

            return Ok(filtered);
        }
    }

    Err(anyhow::anyhow!("No system log file found"))
}
