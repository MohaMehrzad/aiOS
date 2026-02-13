//! container.stop — Stop a running container

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct StopInput {
    name: String,
    #[serde(default = "default_timeout")]
    timeout: u32,
}

fn default_timeout() -> u32 {
    10
}

#[derive(Serialize)]
struct StopOutput {
    success: bool,
    name: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: StopInput = serde_json::from_slice(input).context("Invalid container.stop input")?;

    let output = Command::new("podman")
        .args(["stop", "--time", &req.timeout.to_string(), &req.name])
        .output()
        .context("Failed to run podman stop")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman stop failed: {err}");
    }

    let result = StopOutput {
        success: true,
        name: req.name,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
