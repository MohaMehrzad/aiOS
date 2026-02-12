//! web.api_call — Call external REST APIs with structured request and parse JSON response

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
    body: serde_json::Value,
    #[serde(default)]
    query_params: HashMap<String, String>,
    #[serde(default)]
    auth_bearer: String,
    #[serde(default = "default_timeout")]
    timeout_secs: u32,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_timeout() -> u32 {
    30
}

#[derive(Serialize)]
struct Output {
    status: u32,
    success: bool,
    data: serde_json::Value,
    raw_body: String,
    url: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Build URL with query params
    let url = if input.query_params.is_empty() {
        input.url.clone()
    } else {
        let params: Vec<String> = input
            .query_params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let separator = if input.url.contains('?') { "&" } else { "?" };
        format!("{}{}{}", input.url, separator, params.join("&"))
    };

    let method = input.method.to_uppercase();

    let mut args = vec![
        "-s".to_string(),
        "-S".to_string(),
        "-L".to_string(),
        "--max-time".to_string(),
        input.timeout_secs.to_string(),
        "-w".to_string(),
        "\n__HTTP_STATUS__%{http_code}".to_string(),
        "-X".to_string(),
        method.clone(),
        "-H".to_string(),
        "Accept: application/json".to_string(),
    ];

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

    // Add body for write methods
    if !input.body.is_null() && matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
        let body_str = serde_json::to_string(&input.body)
            .context("Failed to serialize request body")?;
        args.push("-H".to_string());
        args.push("Content-Type: application/json".to_string());
        args.push("-d".to_string());
        args.push(body_str);
    }

    args.push(url.clone());

    let output = Command::new("curl")
        .args(&args)
        .output()
        .with_context(|| format!("Failed to call API: {url}"))?;

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

    // Try to parse response as JSON
    let data = serde_json::from_str::<serde_json::Value>(body.trim())
        .unwrap_or(serde_json::Value::Null);

    let result = Output {
        status,
        success: (200..300).contains(&status),
        data,
        raw_body: body,
        url,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
