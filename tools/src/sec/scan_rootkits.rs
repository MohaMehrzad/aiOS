//! sec.scan_rootkits — Check for hidden processes, suspicious modules, /dev/shm scripts

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct RootkitScanInput {
    #[serde(default = "default_checks")]
    checks: Vec<String>,
}

fn default_checks() -> Vec<String> {
    vec![
        "hidden_processes".into(),
        "suspicious_modules".into(),
        "dev_shm_scripts".into(),
        "proc_anomalies".into(),
    ]
}

#[derive(Serialize)]
struct RootkitScanOutput {
    findings: Vec<RootkitFinding>,
    total_findings: usize,
    clean: bool,
}

#[derive(Serialize)]
struct RootkitFinding {
    check: String,
    severity: String,
    description: String,
    details: Vec<String>,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: RootkitScanInput =
        serde_json::from_slice(input).context("Invalid sec.scan_rootkits input")?;
    let mut findings = Vec::new();

    for check in &req.checks {
        match check.as_str() {
            "hidden_processes" => findings.push(check_hidden_processes()),
            "suspicious_modules" => findings.push(check_suspicious_modules()),
            "dev_shm_scripts" => findings.push(check_dev_shm()),
            "proc_anomalies" => findings.push(check_proc_anomalies()),
            _ => {}
        }
    }

    let total_findings: usize = findings.iter().map(|f| f.details.len()).sum();
    let clean = total_findings == 0;
    let output = RootkitScanOutput {
        findings,
        total_findings,
        clean,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}

fn check_hidden_processes() -> RootkitFinding {
    // Compare ps output with /proc entries
    let ps_pids: Vec<u32> = Command::new("ps")
        .args(["-eo", "pid"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default();

    let proc_pids: Vec<u32> = std::fs::read_dir("/proc")
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default();

    let hidden: Vec<String> = proc_pids
        .iter()
        .filter(|pid| !ps_pids.contains(pid))
        .map(|pid| format!("PID {pid} visible in /proc but not in ps"))
        .collect();

    let severity = if hidden.is_empty() { "low" } else { "critical" }.to_string();
    RootkitFinding {
        check: "hidden_processes".into(),
        severity,
        description: format!("{} potentially hidden processes", hidden.len()),
        details: hidden,
    }
}

fn check_suspicious_modules() -> RootkitFinding {
    let details: Vec<String> = std::fs::read_to_string("/proc/modules")
        .unwrap_or_default()
        .lines()
        .filter(|l| {
            let name = l.split_whitespace().next().unwrap_or("");
            // Flag modules that aren't commonly expected
            name.contains("rootkit") || name.contains("hide") || name.contains("stealth")
        })
        .map(|l| l.to_string())
        .collect();

    let severity = if details.is_empty() {
        "low"
    } else {
        "critical"
    }
    .to_string();
    RootkitFinding {
        check: "suspicious_modules".into(),
        severity,
        description: format!("{} suspicious kernel modules", details.len()),
        details,
    }
}

fn check_dev_shm() -> RootkitFinding {
    let details: Vec<String> = std::fs::read_dir("/dev/shm")
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.ends_with(".sh")
                        || name.ends_with(".py")
                        || name.ends_with(".pl")
                        || name.contains("payload")
                })
                .map(|e| e.path().display().to_string())
                .collect()
        })
        .unwrap_or_default();

    let severity = if details.is_empty() { "low" } else { "high" }.to_string();
    RootkitFinding {
        check: "dev_shm_scripts".into(),
        severity,
        description: format!("{} scripts in /dev/shm", details.len()),
        details,
    }
}

fn check_proc_anomalies() -> RootkitFinding {
    let mut details = Vec::new();

    // Check for deleted but running executables
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.filter_map(|e| e.ok()) {
            let pid = entry.file_name().to_string_lossy().to_string();
            if pid.parse::<u32>().is_err() {
                continue;
            }
            let exe_link = format!("/proc/{pid}/exe");
            if let Ok(target) = std::fs::read_link(&exe_link) {
                let target_str = target.display().to_string();
                if target_str.contains("(deleted)") {
                    details.push(format!("PID {pid}: running deleted binary {target_str}"));
                }
            }
        }
    }

    let severity = if details.is_empty() { "low" } else { "high" }.to_string();
    RootkitFinding {
        check: "proc_anomalies".into(),
        severity,
        description: format!("{} /proc anomalies found", details.len()),
        details,
    }
}
