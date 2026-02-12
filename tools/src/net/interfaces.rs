//! net.interfaces — List network interfaces

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    interfaces: Vec<InterfaceEntry>,
}

#[derive(Serialize)]
struct InterfaceEntry {
    name: String,
    ip: String,
    mac: String,
    status: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let mut interfaces = Vec::new();

    if cfg!(target_os = "macos") {
        // Use ifconfig on macOS
        let output = Command::new("ifconfig")
            .output()
            .context("Failed to execute ifconfig")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut current_name = String::new();
        let mut current_ip = String::new();
        let mut current_mac = String::new();
        let mut current_status = String::new();

        for line in stdout.lines() {
            if !line.starts_with('\t') && !line.starts_with(' ') && line.contains(':') {
                // This is a new interface line
                // Save previous interface if we have one
                if !current_name.is_empty() {
                    interfaces.push(InterfaceEntry {
                        name: current_name.clone(),
                        ip: current_ip.clone(),
                        mac: current_mac.clone(),
                        status: current_status.clone(),
                    });
                }

                // Parse interface name (everything before first colon+space)
                current_name = line.split(':').next().unwrap_or("").trim().to_string();
                current_ip = String::new();
                current_mac = String::new();

                // Check flags for status
                if line.contains("UP") {
                    current_status = "up".to_string();
                } else {
                    current_status = "down".to_string();
                }
            } else {
                let trimmed = line.trim();
                if trimmed.starts_with("inet ") {
                    // IPv4 address
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 2 {
                        current_ip = parts[1].to_string();
                    }
                } else if trimmed.starts_with("ether ") {
                    // MAC address
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 2 {
                        current_mac = parts[1].to_string();
                    }
                }
            }
        }

        // Don't forget the last interface
        if !current_name.is_empty() {
            interfaces.push(InterfaceEntry {
                name: current_name,
                ip: current_ip,
                mac: current_mac,
                status: current_status,
            });
        }
    } else {
        // On Linux, use ip command
        let output = Command::new("ip")
            .args(["-o", "link", "show"])
            .output()
            .context("Failed to execute ip link show")?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }

            let name = parts[1].trim_end_matches(':').to_string();
            let status = if line.contains("UP") {
                "up".to_string()
            } else {
                "down".to_string()
            };

            // Find MAC address
            let mac = parts
                .iter()
                .position(|&p| p == "link/ether")
                .and_then(|i| parts.get(i + 1))
                .map(|s| s.to_string())
                .unwrap_or_default();

            // Get IP address using ip addr show
            let ip = get_linux_ip(&name);

            interfaces.push(InterfaceEntry {
                name,
                ip,
                mac,
                status,
            });
        }
    }

    let result = Output { interfaces };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

#[cfg(not(target_os = "macos"))]
fn get_linux_ip(iface: &str) -> String {
    let output = Command::new("ip")
        .args(["-o", "-4", "addr", "show", iface])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Format: 2: eth0    inet 192.168.1.100/24 ...
            stdout
                .split_whitespace()
                .position(|p| p == "inet")
                .and_then(|i| stdout.split_whitespace().nth(i + 1))
                .map(|s| s.split('/').next().unwrap_or(s).to_string())
                .unwrap_or_default()
        }
        Err(_) => String::new(),
    }
}

#[cfg(target_os = "macos")]
fn get_linux_ip(_iface: &str) -> String {
    String::new()
}
