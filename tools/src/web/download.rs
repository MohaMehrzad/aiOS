//! web.download — Download files from URLs to local paths

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    url: String,
    destination: String,
    /// Create parent directories if they don't exist
    #[serde(default = "default_true")]
    create_dirs: bool,
    /// Maximum download time in seconds
    #[serde(default = "default_timeout")]
    timeout_secs: u32,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u32 {
    120
}

#[derive(Serialize)]
struct Output {
    success: bool,
    url: String,
    destination: String,
    size_bytes: u64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Create parent directories if requested
    if input.create_dirs {
        if let Some(parent) = std::path::Path::new(&input.destination).parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
    }

    // Download using curl
    let output = Command::new("curl")
        .args([
            "-s",
            "-S",
            "-L",
            "--max-time",
            &input.timeout_secs.to_string(),
            "-o",
            &input.destination,
            "-w",
            "%{http_code}",
            &input.url,
        ])
        .output()
        .with_context(|| format!("Failed to download from: {}", input.url))?;

    let status_code = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(0);

    if !(200..300).contains(&status_code) {
        // Clean up partial download on failure
        let _ = std::fs::remove_file(&input.destination);
        anyhow::bail!(
            "Download failed with HTTP status {}: {}",
            status_code,
            input.url
        );
    }

    // Get file size
    let size_bytes = std::fs::metadata(&input.destination)
        .map(|m| m.len())
        .unwrap_or(0);

    let result = Output {
        success: true,
        url: input.url,
        destination: input.destination,
        size_bytes,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
