//! firewall.add_rule — Add a firewall rule

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    chain: String,
    rule: String,
    action: String,
}

#[derive(Serialize)]
struct Output {
    added: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let added = if cfg!(target_os = "macos") {
        add_pf_rule(&input.chain, &input.rule, &input.action)?
    } else {
        add_nft_rule(&input.chain, &input.rule, &input.action)?
    };

    let result = Output { added };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn add_pf_rule(_chain: &str, rule: &str, action: &str) -> Result<bool> {
    // On macOS with PF, we add the rule to the active ruleset
    // First, get current rules
    let current = Command::new("pfctl")
        .args(["-s", "rules"])
        .output()
        .context("Failed to read current PF rules")?;

    let current_rules = String::from_utf8_lossy(&current.stdout).to_string();

    // Construct the new rule line
    // Example: "pass in proto tcp from any to any port 80"
    let new_rule = format!("{} {}", action, rule);

    // Write the combined ruleset to a temporary file and reload
    let combined = format!("{}\n{}\n", current_rules.trim(), new_rule);

    let tmp_path = "/tmp/aios_pf_rules.conf";
    std::fs::write(tmp_path, &combined).context("Failed to write temporary PF rules file")?;

    let output = Command::new("pfctl")
        .args(["-f", tmp_path])
        .output()
        .context("Failed to reload PF rules")?;

    // Clean up temp file
    let _ = std::fs::remove_file(tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // pfctl often writes status info to stderr even on success
        // Check if it's a real error
        if stderr.contains("syntax error") || stderr.contains("error") {
            anyhow::bail!("Failed to add PF rule: {}", stderr.trim());
        }
    }

    Ok(true)
}

fn add_nft_rule(chain: &str, rule: &str, action: &str) -> Result<bool> {
    // On Linux with nftables
    // Assumes a table "filter" exists, which is the common default
    let full_rule = format!("{} {}", rule, action);

    let output = Command::new("nft")
        .args(["add", "rule", "inet", "filter", chain, &full_rule])
        .output()
        .context("Failed to execute nft add rule")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to add nft rule: {}", stderr.trim());
    }

    Ok(true)
}
