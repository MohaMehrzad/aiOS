//! process.spawn — Spawn a new process

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

#[derive(Serialize)]
struct Output {
    pid: u32,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut cmd = Command::new(&input.command);
    cmd.args(&input.args);

    for (key, value) in &input.env {
        cmd.env(key, value);
    }

    // Spawn the process without waiting for it to finish
    let child = cmd.spawn().with_context(|| {
        format!(
            "Failed to spawn process: {} {:?}",
            input.command, input.args
        )
    })?;

    let pid = child.id();

    let result = Output { pid };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
