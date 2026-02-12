//! firewall.delete_rule — Delete a firewall rule by chain and index

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    chain: String,
    index: u32,
}

#[derive(Serialize)]
struct Output {
    deleted: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let deleted = if cfg!(target_os = "macos") {
        delete_pf_rule(&input.chain, input.index)?
    } else {
        delete_nft_rule(&input.chain, input.index)?
    };

    let result = Output { deleted };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn delete_pf_rule(_chain: &str, index: u32) -> Result<bool> {
    // On macOS with PF, we need to remove a rule by its line number
    // Get current rules
    let current = Command::new("pfctl")
        .args(["-s", "rules"])
        .output()
        .context("Failed to read current PF rules")?;

    let current_rules = String::from_utf8_lossy(&current.stdout);
    let lines: Vec<&str> = current_rules.lines().collect();

    if (index as usize) >= lines.len() {
        anyhow::bail!(
            "Rule index {} out of range (only {} rules)",
            index,
            lines.len()
        );
    }

    // Remove the rule at the specified index
    let mut new_rules = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i != index as usize {
            new_rules.push(*line);
        }
    }

    let combined = new_rules.join("\n");
    let tmp_path = "/tmp/aios_pf_rules_del.conf";
    std::fs::write(tmp_path, format!("{}\n", combined))
        .context("Failed to write temporary PF rules file")?;

    let output = Command::new("pfctl")
        .args(["-f", tmp_path])
        .output()
        .context("Failed to reload PF rules")?;

    // Clean up temp file
    let _ = std::fs::remove_file(tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("syntax error") {
            anyhow::bail!("Failed to delete PF rule: {}", stderr.trim());
        }
    }

    Ok(true)
}

fn delete_nft_rule(chain: &str, index: u32) -> Result<bool> {
    // On Linux with nftables, we need the rule handle to delete
    // First, list rules with handles
    let list_output = Command::new("nft")
        .args(["-a", "list", "chain", "inet", "filter", chain])
        .output()
        .context("Failed to list nft rules with handles")?;

    if !list_output.status.success() {
        let stderr = String::from_utf8_lossy(&list_output.stderr);
        anyhow::bail!("Failed to list nft chain {}: {}", chain, stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&list_output.stdout);

    // Find the rule at the given index and extract its handle
    let mut rule_count: u32 = 0;
    let mut target_handle: Option<u32> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        // Rule lines contain "# handle N" at the end
        if trimmed.contains("# handle ") && !trimmed.starts_with("chain ") && !trimmed.starts_with("type ") && !trimmed.starts_with("policy ") {
            if rule_count == index {
                // Extract handle number
                if let Some(handle_str) = trimmed.rsplit("# handle ").next() {
                    target_handle = handle_str.trim().parse::<u32>().ok();
                }
                break;
            }
            rule_count += 1;
        }
    }

    let handle = match target_handle {
        Some(h) => h,
        None => {
            anyhow::bail!(
                "Rule index {} not found in chain {} (only {} rules)",
                index,
                chain,
                rule_count
            );
        }
    };

    // Delete the rule by handle
    let output = Command::new("nft")
        .args([
            "delete",
            "rule",
            "inet",
            "filter",
            chain,
            "handle",
            &handle.to_string(),
        ])
        .output()
        .context("Failed to execute nft delete rule")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to delete nft rule: {}", stderr.trim());
    }

    Ok(true)
}
