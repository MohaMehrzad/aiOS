//! net.http_get — Perform an HTTP GET request

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    url: String,
}

#[derive(Serialize)]
struct Output {
    status: u32,
    body: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Use curl to perform the GET request
    // -s: silent (no progress), -S: show errors
    // -w: write out the HTTP status code after the body
    // -L: follow redirects
    // --max-time: timeout in seconds
    let output = Command::new("curl")
        .args([
            "-s",
            "-S",
            "-L",
            "--max-time",
            "15",
            "-w",
            "\n__HTTP_STATUS__%{http_code}",
            &input.url,
        ])
        .output()
        .with_context(|| format!("Failed to execute curl for URL: {}", input.url))?;

    let raw_output = String::from_utf8_lossy(&output.stdout).to_string();

    // Split the output to extract status code and body
    let (body, status) = if let Some(marker_pos) = raw_output.rfind("__HTTP_STATUS__") {
        let body = raw_output[..marker_pos].to_string();
        let status_str = &raw_output[marker_pos + "__HTTP_STATUS__".len()..];
        let status = status_str.trim().parse::<u32>().unwrap_or(0);
        (body, status)
    } else {
        // Could not find the status marker, use exit code to determine success
        let body = raw_output;
        let status = if output.status.success() { 200 } else { 0 };
        (body, status)
    };

    // Truncate body if it's too large (>1MB)
    let max_body_len = 1024 * 1024;
    let body = if body.len() > max_body_len {
        format!(
            "{}... [truncated, total {} bytes]",
            &body[..max_body_len],
            body.len()
        )
    } else {
        body
    };

    let result = Output { status, body };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
