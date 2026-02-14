//! monitor.cpu — CPU usage, core count, and load averages

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    percent: f64,
    cores: u32,
    load_avg: [f64; 3],
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let (percent, cores, load_avg) = if cfg!(target_os = "macos") {
        get_cpu_macos()?
    } else {
        get_cpu_linux()?
    };

    let result = Output {
        percent,
        cores,
        load_avg,
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn get_cpu_macos() -> Result<(f64, u32, [f64; 3])> {
    // Get core count from sysctl
    let cores_output = Command::new("sysctl")
        .args(["-n", "hw.ncpu"])
        .output()
        .context("Failed to get CPU core count")?;

    let cores = String::from_utf8_lossy(&cores_output.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(1);

    // Get load averages from sysctl
    let load_output = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .context("Failed to get load averages")?;

    let load_str = String::from_utf8_lossy(&load_output.stdout);
    let load_avg = parse_load_avg(&load_str);

    // Get CPU usage from top (snapshot mode)
    // On macOS: top -l 1 -n 0 prints a header with CPU usage
    let top_output = Command::new("top")
        .args(["-l", "2", "-n", "0", "-s", "1"])
        .output()
        .context("Failed to get CPU usage from top")?;

    let top_str = String::from_utf8_lossy(&top_output.stdout);
    let percent = parse_cpu_usage_macos(&top_str);

    Ok((percent, cores, load_avg))
}

fn parse_load_avg(s: &str) -> [f64; 3] {
    // macOS sysctl vm.loadavg format: "{ 1.23 2.34 3.45 }"
    let cleaned = s.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<f64> = cleaned
        .split_whitespace()
        .filter_map(|p| p.parse::<f64>().ok())
        .collect();

    [
        parts.first().copied().unwrap_or(0.0),
        parts.get(1).copied().unwrap_or(0.0),
        parts.get(2).copied().unwrap_or(0.0),
    ]
}

fn parse_cpu_usage_macos(top_output: &str) -> f64 {
    // Look for the line: "CPU usage: X.X% user, Y.Y% sys, Z.Z% idle"
    // Use the last occurrence (second sample is more accurate)
    let mut percent = 0.0;

    for line in top_output.lines() {
        if line.contains("CPU usage:") {
            // Parse user and sys percentages
            let mut user = 0.0_f64;
            let mut sys = 0.0_f64;

            for part in line.split(',') {
                let part = part.trim();
                if part.contains("user") {
                    user = part
                        .split_whitespace()
                        .find_map(|w| w.trim_end_matches('%').parse::<f64>().ok())
                        .unwrap_or(0.0);
                } else if part.contains("sys") {
                    sys = part
                        .split_whitespace()
                        .find_map(|w| w.trim_end_matches('%').parse::<f64>().ok())
                        .unwrap_or(0.0);
                }
            }

            percent = user + sys;
        }
    }

    percent
}

fn get_cpu_linux() -> Result<(f64, u32, [f64; 3])> {
    // Read /proc/cpuinfo for core count
    let cpuinfo =
        std::fs::read_to_string("/proc/cpuinfo").context("Failed to read /proc/cpuinfo")?;

    let cores = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count() as u32;

    // Read /proc/loadavg for load averages
    let loadavg =
        std::fs::read_to_string("/proc/loadavg").context("Failed to read /proc/loadavg")?;

    let load_parts: Vec<f64> = loadavg
        .split_whitespace()
        .take(3)
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();

    let load_avg = [
        load_parts.first().copied().unwrap_or(0.0),
        load_parts.get(1).copied().unwrap_or(0.0),
        load_parts.get(2).copied().unwrap_or(0.0),
    ];

    // Read /proc/stat for CPU usage (two samples, 100ms apart)
    let stat1 = std::fs::read_to_string("/proc/stat").context("Failed to read /proc/stat")?;
    let cpu1 = parse_proc_stat_cpu(&stat1);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let stat2 = std::fs::read_to_string("/proc/stat")
        .context("Failed to read /proc/stat (second sample)")?;
    let cpu2 = parse_proc_stat_cpu(&stat2);

    let total_diff = cpu2.total - cpu1.total;
    let idle_diff = cpu2.idle - cpu1.idle;

    let percent = if total_diff > 0 {
        ((total_diff - idle_diff) as f64 / total_diff as f64) * 100.0
    } else {
        0.0
    };

    Ok((percent, cores, load_avg))
}

struct CpuTimes {
    idle: u64,
    total: u64,
}

fn parse_proc_stat_cpu(stat: &str) -> CpuTimes {
    // First line: "cpu  user nice system idle iowait irq softirq steal guest guest_nice"
    if let Some(line) = stat.lines().next() {
        let values: Vec<u64> = line
            .split_whitespace()
            .skip(1) // skip "cpu"
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();

        let total: u64 = values.iter().sum();
        let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0); // idle + iowait

        CpuTimes { idle, total }
    } else {
        CpuTimes { idle: 0, total: 0 }
    }
}
