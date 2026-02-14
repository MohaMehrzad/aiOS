//! web.http_request — Full HTTP client supporting GET, POST, PUT, DELETE

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    auth_bearer: String,
    #[serde(default = "default_timeout")]
    timeout_secs: u32,
    #[serde(default = "default_true")]
    follow_redirects: bool,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_timeout() -> u32 {
    15
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
struct Output {
    status: u32,
    body: String,
    method: String,
    url: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut args = vec![
        "-s".to_string(),
        "-S".to_string(),
        "--max-time".to_string(),
        input.timeout_secs.to_string(),
        "-w".to_string(),
        "\n__HTTP_STATUS__%{http_code}".to_string(),
        "-X".to_string(),
        input.method.to_uppercase(),
    ];

    if input.follow_redirects {
        args.push("-L".to_string());
    }

    // Add authorization header
    if !input.auth_bearer.is_empty() {
        args.push("-H".to_string());
        args.push(format!("Authorization: Bearer {}", input.auth_bearer));
    }

    // Add custom headers
    for (key, value) in &input.headers {
        args.push("-H".to_string());
        args.push(format!("{key}: {value}"));
    }

    // Add body for POST/PUT/PATCH
    let method_upper = input.method.to_uppercase();
    if !input.body.is_empty() && matches!(method_upper.as_str(), "POST" | "PUT" | "PATCH") {
        args.push("-d".to_string());
        args.push(input.body);

        // Default content-type if not set
        if !input
            .headers
            .keys()
            .any(|k| k.to_lowercase() == "content-type")
        {
            args.push("-H".to_string());
            args.push("Content-Type: application/json".to_string());
        }
    }

    args.push(input.url.clone());

    let output = Command::new("curl")
        .args(&args)
        .output()
        .with_context(|| format!("Failed to execute curl for URL: {}", input.url))?;

    let raw_output = String::from_utf8_lossy(&output.stdout).to_string();

    let (body, status) = if let Some(marker_pos) = raw_output.rfind("__HTTP_STATUS__") {
        let body = raw_output[..marker_pos].to_string();
        let status_str = &raw_output[marker_pos + "__HTTP_STATUS__".len()..];
        let status = status_str.trim().parse::<u32>().unwrap_or(0);
        (body, status)
    } else {
        let status = if output.status.success() { 200 } else { 0 };
        (raw_output, status)
    };

    // Truncate body if too large (>1MB)
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

    let result = Output {
        status,
        body,
        method: method_upper,
        url: input.url,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
