//! container.list — List all containers

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct ListInput {
    #[serde(default)]
    all: bool,
}

#[derive(Serialize)]
struct ContainerInfo {
    id: String,
    name: String,
    image: String,
    status: String,
    ports: String,
}

#[derive(Serialize)]
struct ListOutput {
    containers: Vec<ContainerInfo>,
    total: usize,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: ListInput = serde_json::from_slice(input).context("Invalid container.list input")?;

    let mut cmd = Command::new("podman");
    cmd.args(["ps", "--format", "json"]);
    if req.all {
        cmd.arg("--all");
    }

    let output = cmd.output().context("Failed to run podman ps")?;

    let containers: Vec<ContainerInfo> = if output.status.success() {
        let json_str = String::from_utf8_lossy(&output.stdout);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap_or_default();
        parsed
            .iter()
            .map(|c| ContainerInfo {
                id: c["Id"].as_str().unwrap_or("").chars().take(12).collect(),
                name: c["Names"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                image: c["Image"].as_str().unwrap_or("").to_string(),
                status: c["State"].as_str().unwrap_or("").to_string(),
                ports: c["Ports"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
            })
            .collect()
    } else {
        vec![]
    };

    let total = containers.len();
    let result = ListOutput { containers, total };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
