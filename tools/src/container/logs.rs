//! container.logs — Get container logs

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct LogsInput {
    name: String,
    #[serde(default = "default_tail")]
    tail: u32,
}

fn default_tail() -> u32 {
    100
}

#[derive(Serialize)]
struct LogsOutput {
    success: bool,
    name: String,
    lines: Vec<String>,
    total_lines: usize,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: LogsInput = serde_json::from_slice(input).context("Invalid container.logs input")?;

    let output = Command::new("podman")
        .args(["logs", "--tail", &req.tail.to_string(), &req.name])
        .output()
        .context("Failed to run podman logs")?;

    let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect();

    let total_lines = lines.len();
    let result = LogsOutput {
        success: output.status.success(),
        name: req.name,
        lines,
        total_lines,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
