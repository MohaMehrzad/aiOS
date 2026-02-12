//! monitor.disk — Disk usage for a filesystem

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    #[serde(default = "default_path")]
    path: String,
}

fn default_path() -> String {
    "/".to_string()
}

#[derive(Serialize)]
struct Output {
    total_gb: f64,
    used_gb: f64,
    available_gb: f64,
    percent: f64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = if input.is_empty() {
        Input {
            path: default_path(),
        }
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let path = if input.path.is_empty() {
        "/".to_string()
    } else {
        input.path
    };

    // Use df to get disk usage
    // -k: 1K blocks for consistent parsing
    let output = Command::new("df")
        .args(["-k", &path])
        .output()
        .with_context(|| format!("Failed to execute df for path: {}", path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("df failed for path {}: {}", path, stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.len() < 2 {
        anyhow::bail!("Unexpected df output format");
    }

    // Parse the second line (first data line)
    // Format: Filesystem  1K-blocks  Used  Available  Use%  Mounted-on
    let data_line = lines[1];
    let parts: Vec<&str> = data_line.split_whitespace().collect();

    if parts.len() < 6 {
        anyhow::bail!("Unexpected df output format: insufficient columns");
    }

    // On macOS, df -k columns are:
    // Filesystem 1024-blocks Used Available Capacity iused ifree %iused Mounted
    // We need to find the right indices
    let (total_kb, used_kb, available_kb, percent) = if cfg!(target_os = "macos") {
        // macOS df -k format:
        // Filesystem  1024-blocks  Used  Available  Capacity  ...
        let total = parts[1].parse::<u64>().unwrap_or(0);
        let used = parts[2].parse::<u64>().unwrap_or(0);
        let available = parts[3].parse::<u64>().unwrap_or(0);
        let pct_str = parts[4].trim_end_matches('%');
        let pct = pct_str.parse::<f64>().unwrap_or(0.0);
        (total, used, available, pct)
    } else {
        // Linux df -k format:
        // Filesystem  1K-blocks  Used  Available  Use%  Mounted
        let total = parts[1].parse::<u64>().unwrap_or(0);
        let used = parts[2].parse::<u64>().unwrap_or(0);
        let available = parts[3].parse::<u64>().unwrap_or(0);
        let pct_str = parts[4].trim_end_matches('%');
        let pct = pct_str.parse::<f64>().unwrap_or(0.0);
        (total, used, available, pct)
    };

    // Convert from KB to GB (floating point)
    let kb_to_gb = 1024.0 * 1024.0;
    let total_gb = total_kb as f64 / kb_to_gb;
    let used_gb = used_kb as f64 / kb_to_gb;
    let available_gb = available_kb as f64 / kb_to_gb;

    let result = Output {
        total_gb: (total_gb * 100.0).round() / 100.0,
        used_gb: (used_gb * 100.0).round() / 100.0,
        available_gb: (available_gb * 100.0).round() / 100.0,
        percent,
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}
