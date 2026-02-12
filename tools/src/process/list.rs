//! process.list — List running processes

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    processes: Vec<ProcessEntry>,
}

#[derive(Serialize)]
struct ProcessEntry {
    pid: u32,
    name: String,
    cpu: f64,
    memory: f64,
    status: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let output = Command::new("ps")
        .args(["-eo", "pid,comm,%cpu,%mem,state", "-r"])
        .output()
        .context("Failed to execute ps command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }

        let pid = match parts[0].parse::<u32>() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let name = parts[1].to_string();
        let cpu = parts[2].parse::<f64>().unwrap_or(0.0);
        let memory = parts[3].parse::<f64>().unwrap_or(0.0);

        // Map macOS process state codes to human-readable strings
        let raw_state = parts[4];
        let status = match raw_state.chars().next() {
            Some('R') => "running".to_string(),
            Some('S') => "sleeping".to_string(),
            Some('I') => "idle".to_string(),
            Some('T') => "stopped".to_string(),
            Some('U') => "uninterruptible".to_string(),
            Some('Z') => "zombie".to_string(),
            _ => raw_state.to_string(),
        };

        processes.push(ProcessEntry {
            pid,
            name,
            cpu,
            memory,
            status,
        });
    }

    let result = Output { processes };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
