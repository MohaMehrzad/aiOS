//! monitor.network — Network I/O statistics for an interface

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    #[serde(default = "default_interface")]
    interface: String,
}

fn default_interface() -> String {
    "en0".to_string()
}

#[derive(Serialize)]
struct Output {
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = if input.is_empty() {
        Input {
            interface: default_interface(),
        }
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let iface = if input.interface.is_empty() {
        default_interface()
    } else {
        input.interface
    };

    let result = if cfg!(target_os = "macos") {
        get_network_stats_macos(&iface)?
    } else {
        get_network_stats_linux(&iface)?
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn get_network_stats_macos(interface: &str) -> Result<Output> {
    // Use netstat -I <iface> -b to get byte and packet counts
    let output = Command::new("netstat")
        .args(["-I", interface, "-b"])
        .output()
        .with_context(|| format!("Failed to get netstat for interface {}", interface))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.len() < 2 {
        anyhow::bail!("No data for interface {} from netstat", interface);
    }

    // Header line: Name  Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll
    // Data  line: en0   1500  <link#...>     xx:xx:xx:xx:xx:xx  12345   0    123456789  12345    0   123456789     0
    // Find the data line that matches our interface
    for line in lines.iter().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        if parts[0] == interface {
            // The column positions can vary, but for <link> lines:
            // parts: [Name, Mtu, Network, Address, Ipkts, Ierrs, Ibytes, Opkts, Oerrs, Obytes, Coll]
            // indices:  0     1      2       3       4      5      6       7      8       9      10
            if parts.len() >= 10 {
                let rx_packets = parts[4].parse::<u64>().unwrap_or(0);
                let rx_bytes = parts[6].parse::<u64>().unwrap_or(0);
                let tx_packets = parts[7].parse::<u64>().unwrap_or(0);
                let tx_bytes = parts[9].parse::<u64>().unwrap_or(0);

                return Ok(Output {
                    rx_bytes,
                    tx_bytes,
                    rx_packets,
                    tx_packets,
                });
            }
        }
    }

    // If no matching line found, return zeros
    Ok(Output {
        rx_bytes: 0,
        tx_bytes: 0,
        rx_packets: 0,
        tx_packets: 0,
    })
}

fn get_network_stats_linux(interface: &str) -> Result<Output> {
    // Read from /sys/class/net/<iface>/statistics/
    let base = format!("/sys/class/net/{}/statistics", interface);

    let rx_bytes = read_sys_stat(&format!("{}/rx_bytes", base))?;
    let tx_bytes = read_sys_stat(&format!("{}/tx_bytes", base))?;
    let rx_packets = read_sys_stat(&format!("{}/rx_packets", base))?;
    let tx_packets = read_sys_stat(&format!("{}/tx_packets", base))?;

    Ok(Output {
        rx_bytes,
        tx_bytes,
        rx_packets,
        tx_packets,
    })
}

fn read_sys_stat(path: &str) -> Result<u64> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path))?;

    content
        .trim()
        .parse::<u64>()
        .with_context(|| format!("Failed to parse value from {}", path))
}
