//! container.exec — Execute a command in a running container

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct ExecInput {
    name: String,
    command: Vec<String>,
}

#[derive(Serialize)]
struct ExecOutput {
    success: bool,
    exit_code: i32,
    stdout: String,
    stderr: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: ExecInput = serde_json::from_slice(input).context("Invalid container.exec input")?;

    let mut cmd = Command::new("podman");
    cmd.arg("exec").arg(&req.name);
    for arg in &req.command {
        cmd.arg(arg);
    }

    let output = cmd.output().context("Failed to run podman exec")?;

    let result = ExecOutput {
        success: output.status.success(),
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout)
            .chars()
            .take(10000)
            .collect(),
        stderr: String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(5000)
            .collect(),
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
