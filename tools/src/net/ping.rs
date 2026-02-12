//! net.ping — Ping a remote host

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    host: String,
    #[serde(default = "default_count")]
    count: u32,
}

fn default_count() -> u32 {
    3
}

#[derive(Serialize)]
struct Output {
    success: bool,
    latency_ms: f64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let count = if input.count == 0 { 3 } else { input.count };

    let output = Command::new("ping")
        .args([
            "-c",
            &count.to_string(),
            "-W",
            "5", // timeout in seconds (macOS: -W is in ms on some systems, but -t on macOS)
            &input.host,
        ])
        .output()
        .context("Failed to execute ping command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let success = output.status.success();

    // Parse average latency from the summary line
    // On macOS/Linux: "round-trip min/avg/max/stddev = 1.234/2.345/3.456/0.567 ms"
    let latency_ms = if success {
        parse_ping_latency(&stdout)
    } else {
        0.0
    };

    let result = Output {
        success,
        latency_ms,
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn parse_ping_latency(output: &str) -> f64 {
    for line in output.lines() {
        // Look for the statistics line with min/avg/max/stddev
        if line.contains("min/avg/max") || line.contains("rtt min/avg/max") {
            // Extract the values after the '='
            if let Some(values_str) = line.split('=').last() {
                let values_str = values_str.trim();
                // Format: "1.234/2.345/3.456/0.567 ms"
                let values: Vec<&str> = values_str.split('/').collect();
                if values.len() >= 2 {
                    // The second value is the average
                    if let Ok(avg) = values[1].trim().parse::<f64>() {
                        return avg;
                    }
                }
            }
        }
    }
    0.0
}
