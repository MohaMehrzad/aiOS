//! monitor.memory — Memory usage statistics

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    total_mb: u64,
    used_mb: u64,
    available_mb: u64,
    percent: f64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let result = if cfg!(target_os = "macos") {
        get_memory_macos()?
    } else {
        get_memory_linux()?
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn get_memory_macos() -> Result<Output> {
    // Get total physical memory from sysctl
    let total_output = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .context("Failed to get total memory from sysctl")?;

    let total_bytes: u64 = String::from_utf8_lossy(&total_output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    let total_mb = total_bytes / (1024 * 1024);

    // Get memory usage from vm_stat
    let vm_output = Command::new("vm_stat")
        .output()
        .context("Failed to execute vm_stat")?;

    let vm_str = String::from_utf8_lossy(&vm_output.stdout);

    // Parse vm_stat output
    // Page size is typically 16384 on Apple Silicon or 4096 on Intel
    let page_size = get_page_size();

    let mut pages_free: u64 = 0;
    let mut pages_active: u64 = 0;
    let mut pages_inactive: u64 = 0;
    let mut pages_speculative: u64 = 0;
    let mut pages_wired: u64 = 0;
    let mut pages_compressor: u64 = 0;
    let mut pages_purgeable: u64 = 0;

    for line in vm_str.lines() {
        let line = line.trim();
        if let Some(val) = extract_vm_stat_value(line, "Pages free") {
            pages_free = val;
        } else if let Some(val) = extract_vm_stat_value(line, "Pages active") {
            pages_active = val;
        } else if let Some(val) = extract_vm_stat_value(line, "Pages inactive") {
            pages_inactive = val;
        } else if let Some(val) = extract_vm_stat_value(line, "Pages speculative") {
            pages_speculative = val;
        } else if let Some(val) = extract_vm_stat_value(line, "Pages wired down") {
            pages_wired = val;
        } else if let Some(val) = extract_vm_stat_value(line, "Pages occupied by compressor") {
            pages_compressor = val;
        } else if let Some(val) = extract_vm_stat_value(line, "Pages purgeable") {
            pages_purgeable = val;
        }
    }

    // Calculate used and available memory
    // "Used" = active + wired + compressor
    // "Available" = free + inactive + purgeable + speculative
    let used_pages = pages_active + pages_wired + pages_compressor;
    let available_pages = pages_free + pages_inactive + pages_purgeable + pages_speculative;

    let used_mb = (used_pages * page_size) / (1024 * 1024);
    let available_mb = (available_pages * page_size) / (1024 * 1024);

    let percent = if total_mb > 0 {
        (used_mb as f64 / total_mb as f64) * 100.0
    } else {
        0.0
    };

    Ok(Output {
        total_mb,
        used_mb,
        available_mb,
        percent,
    })
}

fn get_page_size() -> u64 {
    let output = Command::new("sysctl").args(["-n", "hw.pagesize"]).output();

    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u64>()
            .unwrap_or(4096),
        Err(_) => 4096,
    }
}

fn extract_vm_stat_value(line: &str, key: &str) -> Option<u64> {
    if line.starts_with(key) {
        // Format: "Pages free:     12345."
        let value_str = line.split(':').nth(1)?.trim().trim_end_matches('.');
        value_str.parse::<u64>().ok()
    } else {
        None
    }
}

fn get_memory_linux() -> Result<Output> {
    let meminfo =
        std::fs::read_to_string("/proc/meminfo").context("Failed to read /proc/meminfo")?;

    let mut total_kb: u64 = 0;
    let mut available_kb: u64 = 0;
    let mut free_kb: u64 = 0;
    let mut buffers_kb: u64 = 0;
    let mut cached_kb: u64 = 0;

    for line in meminfo.lines() {
        if let Some(val) = extract_meminfo_value(line, "MemTotal") {
            total_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "MemAvailable") {
            available_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "MemFree") {
            free_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "Buffers") {
            buffers_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "Cached") {
            cached_kb = val;
        }
    }

    // If MemAvailable is present, use it; otherwise estimate
    if available_kb == 0 {
        available_kb = free_kb + buffers_kb + cached_kb;
    }

    let total_mb = total_kb / 1024;
    let available_mb = available_kb / 1024;
    let used_mb = total_mb.saturating_sub(available_mb);
    let percent = if total_mb > 0 {
        (used_mb as f64 / total_mb as f64) * 100.0
    } else {
        0.0
    };

    Ok(Output {
        total_mb,
        used_mb,
        available_mb,
        percent,
    })
}

fn extract_meminfo_value(line: &str, key: &str) -> Option<u64> {
    if line.starts_with(key) {
        // Format: "MemTotal:       16384 kB"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            return parts[1].parse::<u64>().ok();
        }
    }
    None
}
