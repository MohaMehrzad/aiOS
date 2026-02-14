//! Hardware detection for aiOS
//!
//! Reads /proc and /sys to determine system capabilities at boot time.

use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

/// Detected hardware information
#[derive(Debug)]
pub struct HardwareInfo {
    pub cpu_count: u32,
    pub cpu_model: String,
    pub ram_mb: u64,
    pub gpu_detected: bool,
    pub gpu_name: String,
    pub storage_devices: Vec<StorageDevice>,
    pub network_interfaces: Vec<NetworkInterface>,
}

#[derive(Debug)]
pub struct StorageDevice {
    pub name: String,
    pub size_gb: u64,
    pub device_type: String,
}

#[derive(Debug)]
pub struct NetworkInterface {
    pub name: String,
    pub mac_address: String,
    pub has_link: bool,
}

/// Detect all hardware present in the system
pub fn detect() -> Result<HardwareInfo> {
    let cpu_count = detect_cpus()?;
    let cpu_model = detect_cpu_model()?;
    let ram_mb = detect_ram()?;
    let (gpu_detected, gpu_name) = detect_gpu();
    let storage_devices = detect_storage();
    let network_interfaces = detect_network();

    let hw = HardwareInfo {
        cpu_count,
        cpu_model,
        ram_mb,
        gpu_detected,
        gpu_name,
        storage_devices,
        network_interfaces,
    };

    // Log detailed hardware info
    info!("CPU: {} x {}", hw.cpu_count, hw.cpu_model);
    info!("RAM: {} MB", hw.ram_mb);
    if hw.gpu_detected {
        info!("GPU: {}", hw.gpu_name);
    } else {
        info!("GPU: None detected (CPU-only mode)");
    }
    for dev in &hw.storage_devices {
        info!(
            "Storage: {} ({} GB, {})",
            dev.name, dev.size_gb, dev.device_type
        );
    }
    for iface in &hw.network_interfaces {
        info!(
            "Network: {} (MAC: {}, Link: {})",
            iface.name, iface.mac_address, iface.has_link
        );
    }

    Ok(hw)
}

fn detect_cpus() -> Result<u32> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let count = cpuinfo
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count();
    Ok(if count == 0 { 1 } else { count as u32 })
}

fn detect_cpu_model() -> Result<String> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model = cpuinfo
        .lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    Ok(model)
}

fn detect_ram() -> Result<u64> {
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let total_kb = meminfo
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .and_then(|line| {
            line.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(0);
    Ok(total_kb / 1024)
}

fn detect_gpu() -> (bool, String) {
    // Check for NVIDIA GPU via /proc/driver/nvidia or PCI
    if Path::new("/proc/driver/nvidia/version").exists() {
        if let Ok(version) = fs::read_to_string("/proc/driver/nvidia/version") {
            return (
                true,
                format!("NVIDIA ({})", version.lines().next().unwrap_or("unknown")),
            );
        }
    }

    // Check PCI devices for GPU
    let pci_path = Path::new("/sys/bus/pci/devices");
    if pci_path.exists() {
        if let Ok(entries) = fs::read_dir(pci_path) {
            for entry in entries.flatten() {
                let class_path = entry.path().join("class");
                if let Ok(class) = fs::read_to_string(&class_path) {
                    let class = class.trim();
                    // 0x030000 = VGA controller, 0x030200 = 3D controller
                    if class.starts_with("0x0302") || class.starts_with("0x0300") {
                        let vendor_path = entry.path().join("vendor");
                        let device_path = entry.path().join("device");
                        let vendor = fs::read_to_string(&vendor_path)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        let device = fs::read_to_string(&device_path)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        let gpu_type = if vendor.contains("10de") {
                            "NVIDIA"
                        } else if vendor.contains("1002") {
                            "AMD"
                        } else if vendor.contains("8086") {
                            "Intel"
                        } else {
                            "Unknown"
                        };
                        return (true, format!("{gpu_type} GPU ({vendor}:{device})"));
                    }
                }
            }
        }
    }

    (false, "None".to_string())
}

fn detect_storage() -> Vec<StorageDevice> {
    let mut devices = Vec::new();
    let block_path = Path::new("/sys/block");
    if !block_path.exists() {
        return devices;
    }

    if let Ok(entries) = fs::read_dir(block_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip loop, ram, and dm devices
            if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("dm-") {
                continue;
            }

            let size_path = entry.path().join("size");
            let size_sectors = fs::read_to_string(&size_path)
                .unwrap_or_default()
                .trim()
                .parse::<u64>()
                .unwrap_or(0);
            let size_gb = size_sectors * 512 / (1024 * 1024 * 1024);

            if size_gb == 0 {
                continue;
            }

            let rotational_path = entry.path().join("queue/rotational");
            let is_rotational = fs::read_to_string(&rotational_path)
                .unwrap_or_default()
                .trim()
                == "1";

            let device_type = if name.starts_with("nvme") {
                "NVMe SSD"
            } else if is_rotational {
                "HDD"
            } else {
                "SSD"
            };

            devices.push(StorageDevice {
                name,
                size_gb,
                device_type: device_type.to_string(),
            });
        }
    }

    devices
}

fn detect_network() -> Vec<NetworkInterface> {
    let mut interfaces = Vec::new();
    let net_path = Path::new("/sys/class/net");
    if !net_path.exists() {
        return interfaces;
    }

    if let Ok(entries) = fs::read_dir(net_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                continue;
            }

            let mac_path = entry.path().join("address");
            let mac_address = fs::read_to_string(&mac_path)
                .unwrap_or_default()
                .trim()
                .to_string();

            let carrier_path = entry.path().join("carrier");
            let has_link = fs::read_to_string(&carrier_path).unwrap_or_default().trim() == "1";

            interfaces.push(NetworkInterface {
                name,
                mac_address,
                has_link,
            });
        }
    }

    interfaces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_returns_ok() {
        // Should work even on non-Linux (returns defaults)
        let result = detect();
        assert!(result.is_ok());
    }
}
