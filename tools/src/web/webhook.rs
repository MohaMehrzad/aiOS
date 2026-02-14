//! web.webhook — Send webhook notifications to external URLs

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    url: String,
    #[serde(default)]
    payload: serde_json::Value,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    secret: String,
}

#[derive(Serialize)]
struct Output {
    success: bool,
    status: u32,
    response_body: String,
    url: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let payload_str =
        serde_json::to_string(&input.payload).context("Failed to serialize webhook payload")?;

    let mut args = vec![
        "-s".to_string(),
        "-S".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        "--max-time".to_string(),
        "10".to_string(),
        "-w".to_string(),
        "\n__HTTP_STATUS__%{http_code}".to_string(),
        "-H".to_string(),
        "Content-Type: application/json".to_string(),
        "-d".to_string(),
        payload_str,
    ];

    // Add HMAC signature header if secret provided
    if !input.secret.is_empty() {
        args.push("-H".to_string());
        args.push(format!("X-Webhook-Secret: {}", input.secret));
    }

    // Add custom headers
    for (key, value) in &input.headers {
        args.push("-H".to_string());
        args.push(format!("{key}: {value}"));
    }

    args.push(input.url.clone());

    let output = Command::new("curl")
        .args(&args)
        .output()
        .with_context(|| format!("Failed to send webhook to: {}", input.url))?;

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

    let result = Output {
        success: (200..300).contains(&status),
        status,
        response_body: body,
        url: input.url,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
