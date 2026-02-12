//! firewall.rules — List firewall rules

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    rules: Vec<RuleEntry>,
}

#[derive(Serialize)]
struct RuleEntry {
    chain: String,
    rule: String,
    action: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let rules = if cfg!(target_os = "macos") {
        list_pf_rules()?
    } else {
        list_nft_rules()?
    };

    let result = Output { rules };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn list_pf_rules() -> Result<Vec<RuleEntry>> {
    // On macOS, use pfctl to list rules
    let output = Command::new("pfctl")
        .args(["-s", "rules"])
        .output()
        .context("Failed to execute pfctl. Ensure you have sufficient privileges.")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // pfctl outputs rules to stdout and status to stderr
    let combined = if stdout.is_empty() {
        stderr.to_string()
    } else {
        stdout.to_string()
    };

    let mut rules = Vec::new();

    for line in combined.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // PF rules look like: "pass in on en0 proto tcp from any to any port 80"
        // or "block drop in all"
        let action = if line.starts_with("pass") {
            "pass".to_string()
        } else if line.starts_with("block") {
            "block".to_string()
        } else if line.starts_with("scrub") {
            "scrub".to_string()
        } else if line.starts_with("nat") {
            "nat".to_string()
        } else if line.starts_with("rdr") {
            "rdr".to_string()
        } else {
            "other".to_string()
        };

        // Determine direction as chain
        let chain = if line.contains(" in ") {
            "input".to_string()
        } else if line.contains(" out ") {
            "output".to_string()
        } else {
            "filter".to_string()
        };

        rules.push(RuleEntry {
            chain,
            rule: line.to_string(),
            action,
        });
    }

    Ok(rules)
}

fn list_nft_rules() -> Result<Vec<RuleEntry>> {
    // On Linux, use nft to list rules
    let output = Command::new("nft")
        .args(["list", "ruleset"])
        .output()
        .context("Failed to execute nft. Ensure nftables is installed.")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nft list ruleset failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rules = Vec::new();
    let mut current_chain = String::new();

    for line in stdout.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("chain ") {
            // Parse chain name: "chain input {"
            current_chain = trimmed
                .strip_prefix("chain ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("unknown")
                .to_string();
        } else if !current_chain.is_empty()
            && !trimmed.is_empty()
            && !trimmed.starts_with("type ")
            && !trimmed.starts_with("policy ")
            && trimmed != "}"
            && !trimmed.starts_with("table ")
            && !trimmed.starts_with("chain ")
        {
            // This is a rule line
            let action = if trimmed.contains("accept") {
                "accept".to_string()
            } else if trimmed.contains("drop") {
                "drop".to_string()
            } else if trimmed.contains("reject") {
                "reject".to_string()
            } else if trimmed.contains("log") {
                "log".to_string()
            } else {
                "other".to_string()
            };

            rules.push(RuleEntry {
                chain: current_chain.clone(),
                rule: trimmed.to_string(),
                action,
            });
        }

        if trimmed == "}" {
            // End of chain or table block — only clear chain if we're at chain level
            if !current_chain.is_empty() {
                current_chain = String::new();
            }
        }
    }

    Ok(rules)
}
