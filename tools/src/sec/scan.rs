//! sec.scan — Security scan: open ports, world-writable files, SUID binaries, weak perms

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct ScanInput {
    #[serde(default = "default_checks")]
    checks: Vec<String>,
}

fn default_checks() -> Vec<String> {
    vec![
        "open_ports".into(),
        "world_writable".into(),
        "suid_binaries".into(),
        "weak_perms".into(),
    ]
}

#[derive(Serialize)]
struct ScanOutput {
    findings: Vec<ScanFinding>,
    total_findings: usize,
    risk_level: String,
}

#[derive(Serialize)]
struct ScanFinding {
    check: String,
    severity: String,
    description: String,
    details: Vec<String>,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: ScanInput = serde_json::from_slice(input).context("Invalid sec.scan input")?;
    let mut findings = Vec::new();

    for check in &req.checks {
        match check.as_str() {
            "open_ports" => findings.push(scan_open_ports()),
            "world_writable" => findings.push(scan_world_writable()),
            "suid_binaries" => findings.push(scan_suid_binaries()),
            "weak_perms" => findings.push(scan_weak_perms()),
            _ => findings.push(ScanFinding {
                check: check.clone(),
                severity: "info".into(),
                description: format!("Unknown check: {check}"),
                details: vec![],
            }),
        }
    }

    let max_severity = findings
        .iter()
        .map(|f| match f.severity.as_str() {
            "critical" => 3,
            "high" => 2,
            "medium" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0);

    let risk_level = match max_severity {
        3 => "critical",
        2 => "high",
        1 => "medium",
        _ => "low",
    }
    .to_string();

    let total_findings = findings.iter().map(|f| f.details.len()).sum();
    let output = ScanOutput {
        findings,
        total_findings,
        risk_level,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}

fn scan_open_ports() -> ScanFinding {
    let details = Command::new("ss")
        .args(["-tlnp"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .skip(1)
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let severity = if details.len() > 10 { "medium" } else { "low" }.to_string();
    ScanFinding {
        check: "open_ports".into(),
        severity,
        description: format!("{} listening ports found", details.len()),
        details,
    }
}

fn scan_world_writable() -> ScanFinding {
    let details = Command::new("find")
        .args([
            "/etc",
            "/var",
            "-maxdepth",
            "3",
            "-perm",
            "-o+w",
            "-type",
            "f",
        ])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .take(50)
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let severity = if details.is_empty() { "low" } else { "high" }.to_string();
    ScanFinding {
        check: "world_writable".into(),
        severity,
        description: format!("{} world-writable files found", details.len()),
        details,
    }
}

fn scan_suid_binaries() -> ScanFinding {
    let details = Command::new("find")
        .args(["/usr", "/bin", "/sbin", "-perm", "-4000", "-type", "f"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .take(50)
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let severity = if details.len() > 20 { "medium" } else { "low" }.to_string();
    ScanFinding {
        check: "suid_binaries".into(),
        severity,
        description: format!("{} SUID binaries found", details.len()),
        details,
    }
}

fn scan_weak_perms() -> ScanFinding {
    let paths_to_check = [
        "/etc/shadow",
        "/etc/passwd",
        "/etc/ssh/sshd_config",
        "/var/lib/aios/data",
    ];

    let mut details = Vec::new();
    for path in &paths_to_check {
        if let Ok(metadata) = std::fs::metadata(path) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = metadata.permissions().mode();
                if mode & 0o077 != 0 && *path != "/etc/passwd" {
                    details.push(format!("{path}: mode {mode:04o} (too permissive)"));
                }
            }
        }
    }

    let severity = if details.is_empty() { "low" } else { "medium" }.to_string();
    ScanFinding {
        check: "weak_perms".into(),
        severity,
        description: format!("{} weak permission issues found", details.len()),
        details,
    }
}
