//! Plugin Templates — pre-built plugin recipes
//!
//! Provides ready-made plugin templates that can be instantiated
//! via `plugin.from_template`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct TemplateInput {
    template: String,
    #[serde(default)]
    config: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct TemplateOutput {
    success: bool,
    plugin_name: String,
    tool_name: String,
}

/// Available templates: (name, description, code, capabilities)
const TEMPLATES: &[(&str, &str, &str, &[&str])] = &[
    (
        "web_scraper",
        "Scrape web page content and extract structured data",
        r#"import json, urllib.request

def main(input_data: dict) -> dict:
    url = input_data.get("url", "")
    if not url:
        return {"error": "No URL provided"}
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "aiOS/1.0"})
        with urllib.request.urlopen(req, timeout=30) as response:
            content = response.read().decode("utf-8", errors="replace")
            return {"url": url, "status": response.status, "content_length": len(content), "content": content[:5000]}
    except Exception as e:
        return {"error": str(e), "url": url}
"#,
        &["net_read"],
    ),
    (
        "log_analyzer",
        "Analyze log files for patterns, errors, and anomalies",
        r#"import re
from collections import Counter
from pathlib import Path

def main(input_data: dict) -> dict:
    log_path = input_data.get("path", "/var/log/syslog")
    max_lines = input_data.get("max_lines", 1000)
    pattern = input_data.get("pattern", r"(ERROR|CRITICAL|FATAL|WARN)")
    try:
        lines = Path(log_path).read_text().splitlines()[-max_lines:]
    except Exception as e:
        return {"error": f"Cannot read {log_path}: {e}"}
    matches = [l for l in lines if re.search(pattern, l, re.IGNORECASE)]
    level_counts = Counter()
    for line in matches:
        for level in ["ERROR", "CRITICAL", "FATAL", "WARN"]:
            if level in line.upper():
                level_counts[level] += 1
    return {"total_lines": len(lines), "matching_lines": len(matches), "level_counts": dict(level_counts), "recent_matches": matches[-10:]}
"#,
        &["fs_read"],
    ),
    (
        "file_processor",
        "Process files with custom transformations",
        r#"from pathlib import Path

def main(input_data: dict) -> dict:
    path = input_data.get("path", "")
    operation = input_data.get("operation", "stats")
    if not path:
        return {"error": "No path provided"}
    p = Path(path)
    if not p.exists():
        return {"error": f"Path does not exist: {path}"}
    if operation == "stats":
        if p.is_file():
            content = p.read_text(errors="replace")
            return {"path": path, "size": p.stat().st_size, "lines": len(content.splitlines()), "words": len(content.split()), "chars": len(content)}
        elif p.is_dir():
            files = list(p.rglob("*"))
            return {"path": path, "total_files": sum(1 for f in files if f.is_file()), "total_dirs": sum(1 for f in files if f.is_dir()), "total_size": sum(f.stat().st_size for f in files if f.is_file())}
    return {"error": f"Unknown operation: {operation}"}
"#,
        &["fs_read"],
    ),
    (
        "api_client",
        "Make API calls with JSON request/response handling",
        r#"import json, urllib.request

def main(input_data: dict) -> dict:
    url = input_data.get("url", "")
    method = input_data.get("method", "GET").upper()
    headers = input_data.get("headers", {})
    body = input_data.get("body")
    if not url:
        return {"error": "No URL provided"}
    headers.setdefault("Content-Type", "application/json")
    headers.setdefault("User-Agent", "aiOS/1.0")
    data = json.dumps(body).encode() if body else None
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            content = response.read().decode("utf-8", errors="replace")
            try:
                parsed = json.loads(content)
            except json.JSONDecodeError:
                parsed = content
            return {"status": response.status, "headers": dict(response.headers), "body": parsed}
    except Exception as e:
        return {"error": str(e)}
"#,
        &["net_read", "net_write"],
    ),
];

/// Execute plugin.from_template
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: TemplateInput = serde_json::from_slice(input).context("Invalid template input")?;

    let template = TEMPLATES
        .iter()
        .find(|(name, _, _, _)| *name == req.template)
        .ok_or_else(|| {
            let available: Vec<&str> = TEMPLATES.iter().map(|(n, _, _, _)| *n).collect();
            anyhow::anyhow!(
                "Unknown template '{}'. Available: {:?}",
                req.template,
                available
            )
        })?;

    let (name, description, code, capabilities) = template;

    let create_input = serde_json::json!({
        "name": name,
        "description": description,
        "code": code,
        "capabilities": capabilities,
        "dependencies": [],
    });

    let input_bytes = serde_json::to_vec(&create_input)?;
    super::create::execute(&input_bytes)?;

    let output = TemplateOutput {
        success: true,
        plugin_name: name.to_string(),
        tool_name: format!("plugin.{}", name),
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}

/// List available templates
pub fn list_templates() -> Vec<(String, String)> {
    TEMPLATES
        .iter()
        .map(|(name, desc, _, _)| (name.to_string(), desc.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_templates() {
        let templates = list_templates();
        assert_eq!(templates.len(), 4);
        assert!(templates.iter().any(|(name, _)| name == "web_scraper"));
    }
}
