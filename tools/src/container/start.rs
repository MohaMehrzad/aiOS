//! container.start — Start a container

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct StartInput {
    name: String,
}

#[derive(Serialize)]
struct StartOutput {
    success: bool,
    name: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: StartInput = serde_json::from_slice(input).context("Invalid container.start input")?;

    let output = Command::new("podman")
        .args(["start", &req.name])
        .output()
        .context("Failed to run podman start")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman start failed: {err}");
    }

    let result = StartOutput {
        success: true,
        name: req.name,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
