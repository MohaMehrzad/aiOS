//! container.create — Create a new Podman container

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct CreateInput {
    image: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    ports: Vec<String>,
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
    #[serde(default)]
    volumes: Vec<String>,
}

#[derive(Serialize)]
struct CreateOutput {
    success: bool,
    container_id: String,
    name: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: CreateInput =
        serde_json::from_slice(input).context("Invalid container.create input")?;

    let mut cmd = Command::new("podman");
    cmd.arg("create");

    if !req.name.is_empty() {
        cmd.args(["--name", &req.name]);
    }

    for port in &req.ports {
        cmd.args(["-p", port]);
    }

    for (key, val) in &req.env {
        cmd.args(["-e", &format!("{key}={val}")]);
    }

    for vol in &req.volumes {
        cmd.args(["-v", vol]);
    }

    cmd.arg(&req.image);

    let output = cmd.output().context("Failed to run podman create")?;

    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman create failed: {err}");
    }

    let result = CreateOutput {
        success: true,
        container_id: container_id.clone(),
        name: if req.name.is_empty() {
            container_id
        } else {
            req.name
        },
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
