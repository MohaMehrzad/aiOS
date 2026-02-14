//! hw.info — System hardware information

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    cpu: String,
    ram_mb: u64,
    gpu: String,
    storage: Vec<StorageDevice>,
}

#[derive(Serialize)]
struct StorageDevice {
    name: String,
    size_gb: f64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let result = if cfg!(target_os = "macos") {
        get_hw_info_macos()?
    } else {
        get_hw_info_linux()?
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn get_hw_info_macos() -> Result<Output> {
    // CPU model
    let cpu_output = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .context("Failed to get CPU info")?;

    let cpu = String::from_utf8_lossy(&cpu_output.stdout)
        .trim()
        .to_string();

    // If the above fails (e.g., on Apple Silicon), try the chip name
    let cpu = if cpu.is_empty() {
        let chip = Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand"])
            .output();
        match chip {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if s.is_empty() {
                    // Fall back to system_profiler
                    get_cpu_from_system_profiler()
                } else {
                    s
                }
            }
            Err(_) => get_cpu_from_system_profiler(),
        }
    } else {
        cpu
    };

    // RAM
    let ram_output = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .context("Failed to get RAM info")?;

    let ram_bytes: u64 = String::from_utf8_lossy(&ram_output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    let ram_mb = ram_bytes / (1024 * 1024);

    // GPU
    let gpu = get_gpu_macos();

    // Storage devices
    let storage = get_storage_macos()?;

    Ok(Output {
        cpu,
        ram_mb,
        gpu,
        storage,
    })
}

fn get_cpu_from_system_profiler() -> String {
    let output = Command::new("system_profiler")
        .args(["SPHardwareDataType"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Chip:") || trimmed.starts_with("Processor Name:") {
                    return trimmed
                        .split(':')
                        .nth(1)
                        .unwrap_or("Unknown")
                        .trim()
                        .to_string();
                }
            }
            "Unknown".to_string()
        }
        Err(_) => "Unknown".to_string(),
    }
}

fn get_gpu_macos() -> String {
    let output = Command::new("system_profiler")
        .args(["SPDisplaysDataType"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Chipset Model:") || trimmed.starts_with("Chip:") {
                    return trimmed
                        .split(':')
                        .nth(1)
                        .unwrap_or("Unknown")
                        .trim()
                        .to_string();
                }
            }
            // On Apple Silicon, the GPU is part of the chip
            "Integrated (see CPU)".to_string()
        }
        Err(_) => "Unknown".to_string(),
    }
}

fn get_storage_macos() -> Result<Vec<StorageDevice>> {
    let _output = Command::new("diskutil")
        .args(["list", "-plist"])
        .output()
        .context("Failed to execute diskutil")?;

    // Parse the simpler text output instead
    let text_output = Command::new("diskutil")
        .arg("list")
        .output()
        .context("Failed to execute diskutil list")?;

    let stdout = String::from_utf8_lossy(&text_output.stdout);
    let mut devices = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        // Look for physical disk lines like "/dev/disk0 (internal):"
        if line.starts_with("/dev/disk") && line.contains(':') {
            let name = line.split(':').next().unwrap_or(line).trim().to_string();

            // Get size for this disk
            let size_gb = get_disk_size_macos(&name);

            devices.push(StorageDevice { name, size_gb });
        }
    }

    // If diskutil didn't find anything, fall back to df
    if devices.is_empty() {
        let df_output = Command::new("df")
            .args(["-g"])
            .output()
            .context("Failed to execute df")?;

        let df_stdout = String::from_utf8_lossy(&df_output.stdout);
        for line in df_stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0].to_string();
                let size_gb = parts[1].parse::<f64>().unwrap_or(0.0);
                if size_gb > 0.0 {
                    devices.push(StorageDevice { name, size_gb });
                }
            }
        }
    }

    Ok(devices)
}

fn get_disk_size_macos(disk: &str) -> f64 {
    let output = Command::new("diskutil").args(["info", disk]).output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Disk Size:") || trimmed.starts_with("Total Size:") {
                    // Format: "Disk Size:  500.1 GB (500107862016 Bytes)"
                    let after_colon = trimmed.split(':').nth(1).unwrap_or("").trim();
                    let parts: Vec<&str> = after_colon.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let value = parts[0].parse::<f64>().unwrap_or(0.0);
                        let unit = parts[1].to_uppercase();
                        return match unit.as_str() {
                            "TB" => value * 1024.0,
                            "GB" => value,
                            "MB" => value / 1024.0,
                            _ => value,
                        };
                    }
                }
            }
            0.0
        }
        Err(_) => 0.0,
    }
}

fn get_hw_info_linux() -> Result<Output> {
    // CPU model from /proc/cpuinfo
    let cpuinfo =
        std::fs::read_to_string("/proc/cpuinfo").context("Failed to read /proc/cpuinfo")?;

    let cpu = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    // RAM from /proc/meminfo
    let meminfo =
        std::fs::read_to_string("/proc/meminfo").context("Failed to read /proc/meminfo")?;

    let ram_kb = meminfo
        .lines()
        .find(|l| l.starts_with("MemTotal"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let ram_mb = ram_kb / 1024;

    // GPU from lspci
    let gpu = get_gpu_linux();

    // Storage from lsblk
    let storage = get_storage_linux()?;

    Ok(Output {
        cpu,
        ram_mb,
        gpu,
        storage,
    })
}

fn get_gpu_linux() -> String {
    let output = Command::new("lspci").output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.contains("VGA") || line.contains("3D") || line.contains("Display") {
                    // Format: "01:00.0 VGA compatible controller: NVIDIA Corporation ..."
                    if let Some(desc) = line.split(':').last() {
                        return desc.trim().to_string();
                    }
                }
            }
            "Unknown".to_string()
        }
        Err(_) => "Unknown".to_string(),
    }
}

fn get_storage_linux() -> Result<Vec<StorageDevice>> {
    let output = Command::new("lsblk")
        .args(["-bno", "NAME,SIZE,TYPE"])
        .output()
        .context("Failed to execute lsblk")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[2] == "disk" {
            let name = format!("/dev/{}", parts[0]);
            let size_bytes = parts[1].parse::<u64>().unwrap_or(0);
            let size_gb = size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

            devices.push(StorageDevice {
                name,
                size_gb: (size_gb * 100.0).round() / 100.0,
            });
        }
    }

    Ok(devices)
}
