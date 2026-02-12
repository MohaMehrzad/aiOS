//! net.dns — DNS lookup

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::ToSocketAddrs;

#[derive(Deserialize)]
struct Input {
    hostname: String,
}

#[derive(Serialize)]
struct Output {
    addresses: Vec<String>,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Use std::net to resolve the hostname
    // ToSocketAddrs requires a port, so we append :0
    let lookup_host = format!("{}:0", input.hostname);

    let addresses: Vec<String> = match lookup_host.to_socket_addrs() {
        Ok(addrs) => addrs.map(|addr| addr.ip().to_string()).collect(),
        Err(e) => {
            // Fallback: try using the host command
            match resolve_with_host_command(&input.hostname) {
                Some(addrs) => addrs,
                None => {
                    return Err(anyhow::anyhow!(
                        "DNS resolution failed for {}: {}",
                        input.hostname,
                        e
                    ));
                }
            }
        }
    };

    // Deduplicate addresses (to_socket_addrs can return duplicates)
    let mut unique_addresses: Vec<String> = Vec::new();
    for addr in addresses {
        if !unique_addresses.contains(&addr) {
            unique_addresses.push(addr);
        }
    }

    let result = Output {
        addresses: unique_addresses,
    };

    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn resolve_with_host_command(hostname: &str) -> Option<Vec<String>> {
    use std::process::Command;

    let output = Command::new("host").arg(hostname).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut addresses = Vec::new();

    for line in stdout.lines() {
        // "hostname has address 1.2.3.4"
        if line.contains("has address") {
            if let Some(addr) = line.split_whitespace().last() {
                addresses.push(addr.to_string());
            }
        }
        // "hostname has IPv6 address ::1"
        if line.contains("has IPv6 address") {
            if let Some(addr) = line.split_whitespace().last() {
                addresses.push(addr.to_string());
            }
        }
    }

    if addresses.is_empty() {
        None
    } else {
        Some(addresses)
    }
}
