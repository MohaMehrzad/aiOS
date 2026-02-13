//! self.inspect — Inspect aiOS source code, version, and capabilities
//! self.health — Check system health

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

// ── self.inspect ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct InspectInput {
    /// What to inspect: "version", "components", "config", "source"
    #[serde(default = "default_inspect_what")]
    #[allow(dead_code)]
    what: String,
    /// Source root path (defaults to /opt/aios)
    #[serde(default = "default_source_path")]
    source_path: String,
}

fn default_inspect_what() -> String {
    "version".to_string()
}

fn default_source_path() -> String {
    "/opt/aios".to_string()
}

#[derive(Serialize)]
struct InspectOutput {
    version: String,
    components: Vec<ComponentInfo>,
    git_revision: String,
    git_branch: String,
    source_path: String,
}

#[derive(Serialize)]
struct ComponentInfo {
    name: String,
    path: String,
    binary_exists: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: InspectInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Get git revision
    let git_rev = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(&input.source_path)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let git_branch = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(&input.source_path)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // Read version from Cargo workspace
    let version = std::fs::read_to_string(format!("{}/Cargo.toml", input.source_path))
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.trim().starts_with("version"))
                .and_then(|l| l.split('"').nth(1))
                .map(|v| v.to_string())
        })
        .unwrap_or_else(|| "0.1.0".to_string());

    // List components
    let components = vec![
        check_component("aios-orchestrator", &input.source_path, "agent-core"),
        check_component("aios-tools", &input.source_path, "tools"),
        check_component("aios-memory", &input.source_path, "memory"),
        check_component("aios-runtime", &input.source_path, "runtime"),
        check_component("aios-api-gateway", &input.source_path, "api-gateway"),
        check_component("aios-init", &input.source_path, "initd"),
    ];

    let result = InspectOutput {
        version,
        components,
        git_revision: git_rev,
        git_branch,
        source_path: input.source_path,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn check_component(name: &str, source_path: &str, dir: &str) -> ComponentInfo {
    let path = format!("{source_path}/{dir}");
    let binary_path = format!("{source_path}/target/release/{name}");
    ComponentInfo {
        name: name.to_string(),
        path,
        binary_exists: std::path::Path::new(&binary_path).exists(),
    }
}

// ── self.health ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct HealthInput {
    /// Check connectivity to services
    #[serde(default = "default_true")]
    check_services: bool,
    /// Check disk space
    #[serde(default = "default_true")]
    check_disk: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
struct HealthOutput {
    healthy: bool,
    services: Vec<ServiceHealth>,
    disk_ok: bool,
    disk_usage_percent: f64,
    uptime_seconds: u64,
    issues: Vec<String>,
}

#[derive(Serialize)]
struct ServiceHealth {
    name: String,
    port: u16,
    reachable: bool,
}

pub fn execute_health(input: &[u8]) -> Result<Vec<u8>> {
    let input: HealthInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut issues = Vec::new();
    let mut services = Vec::new();

    // Check service ports
    if input.check_services {
        let service_ports = [
            ("orchestrator", 50051),
            ("tools", 50052),
            ("memory", 50053),
            ("runtime", 50055),
            ("api-gateway", 50054),
            ("management-console", 9090),
        ];

        for (name, port) in service_ports {
            let reachable = check_port(port);
            if !reachable {
                issues.push(format!("Service {name} not reachable on port {port}"));
            }
            services.push(ServiceHealth {
                name: name.to_string(),
                port,
                reachable,
            });
        }
    }

    // Check disk space
    let (disk_ok, disk_usage_percent) = if input.check_disk {
        let output = Command::new("df")
            .args(["-h", "/"])
            .output()
            .ok();

        if let Some(output) = output {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            let percent = text
                .lines()
                .nth(1)
                .and_then(|l| {
                    l.split_whitespace()
                        .find(|w| w.ends_with('%'))
                        .and_then(|w| w.trim_end_matches('%').parse::<f64>().ok())
                })
                .unwrap_or(0.0);

            if percent > 90.0 {
                issues.push(format!("Disk usage critically high: {percent}%"));
            }
            (percent < 95.0, percent)
        } else {
            (true, 0.0)
        }
    } else {
        (true, 0.0)
    };

    // Get uptime
    let uptime_seconds = get_uptime();

    let healthy = issues.is_empty();

    let result = HealthOutput {
        healthy,
        services,
        disk_ok,
        disk_usage_percent,
        uptime_seconds,
        issues,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn check_port(port: u16) -> bool {
    std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok()
}

fn get_uptime() -> u64 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
            .map(|s| s as u64)
            .unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}
