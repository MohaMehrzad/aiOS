//! process.info — Get detailed information about a process

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    pid: u32,
}

#[derive(Serialize)]
struct Output {
    pid: u32,
    name: String,
    cmdline: String,
    cpu: f64,
    memory: f64,
    threads: u32,
    started_at: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Use ps to get process details on macOS
    // -p selects by PID, -o specifies output columns
    let output = Command::new("ps")
        .args([
            "-p",
            &input.pid.to_string(),
            "-o",
            "pid,comm,%cpu,%mem,state,lstart,command",
            "-ww", // wide output to avoid truncation
        ])
        .output()
        .context("Failed to execute ps command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.len() < 2 {
        anyhow::bail!("Process {} not found", input.pid);
    }

    // Parse the first data line
    // Note: lstart has spaces (e.g. "Wed Feb 12 10:30:00 2026"), so we need careful parsing
    let line = lines[1];
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.len() < 7 {
        anyhow::bail!("Unexpected ps output format for pid {}", input.pid);
    }

    let pid = parts[0].parse::<u32>().unwrap_or(input.pid);
    let name = parts[1].to_string();
    let cpu = parts[2].parse::<f64>().unwrap_or(0.0);
    let memory = parts[3].parse::<f64>().unwrap_or(0.0);
    // parts[4] is state
    // parts[5..10] is lstart (day_of_week month day HH:MM:SS year)
    let started_at = if parts.len() >= 10 {
        format!(
            "{} {} {} {} {}",
            parts[5], parts[6], parts[7], parts[8], parts[9]
        )
    } else {
        "unknown".to_string()
    };
    // Everything after lstart is the full command
    let cmdline = if parts.len() > 10 {
        parts[10..].join(" ")
    } else {
        name.clone()
    };

    // Get thread count using a separate ps call
    let thread_output = Command::new("ps")
        .args(["-M", "-p", &input.pid.to_string()])
        .output();

    let threads = match thread_output {
        Ok(ref out) => {
            let s = String::from_utf8_lossy(&out.stdout);
            // Each line after the header is a thread
            let count = s.lines().count();
            if count > 1 {
                (count - 1) as u32
            } else {
                1
            }
        }
        Err(_) => 1,
    };

    let result = Output {
        pid,
        name,
        cmdline,
        cpu,
        memory,
        threads,
        started_at,
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}
