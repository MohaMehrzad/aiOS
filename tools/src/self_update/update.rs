//! self.update — Pull latest source and apply updates
//! self.rebuild — Rebuild components from source

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

// ── self.update ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct UpdateInput {
    /// Source root path
    #[serde(default = "default_source_path")]
    source_path: String,
    /// Remote name to pull from
    #[serde(default = "default_remote")]
    remote: String,
    /// Branch to pull
    #[serde(default = "default_branch")]
    branch: String,
}

fn default_source_path() -> String {
    "/opt/aios".to_string()
}

fn default_remote() -> String {
    "origin".to_string()
}

fn default_branch() -> String {
    "main".to_string()
}

#[derive(Serialize)]
struct UpdateOutput {
    success: bool,
    previous_rev: String,
    current_rev: String,
    files_changed: usize,
    output: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: UpdateInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Get current revision before pull
    let prev_rev = get_rev(&input.source_path);

    // Pull latest
    let pull_output = Command::new("git")
        .args(["pull", &input.remote, &input.branch])
        .current_dir(&input.source_path)
        .output()
        .context("Failed to execute git pull")?;

    if !pull_output.status.success() {
        let stderr = String::from_utf8_lossy(&pull_output.stderr);
        anyhow::bail!("git pull failed: {stderr}");
    }

    let pull_stdout = String::from_utf8_lossy(&pull_output.stdout).to_string();

    // Get new revision
    let current_rev = get_rev(&input.source_path);

    // Count files changed
    let diff_output = Command::new("git")
        .args(["diff", "--name-only", &prev_rev, &current_rev])
        .current_dir(&input.source_path)
        .output()
        .ok();

    let files_changed = diff_output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .count()
        })
        .unwrap_or(0);

    let result = UpdateOutput {
        success: true,
        previous_rev: prev_rev,
        current_rev,
        files_changed,
        output: pull_stdout,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn get_rev(path: &str) -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(path)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

// ── self.rebuild ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct RebuildInput {
    /// Source root path
    #[serde(default = "default_source_path")]
    source_path: String,
    /// Components to rebuild (empty = all)
    #[serde(default)]
    components: Vec<String>,
    /// Build in release mode
    #[serde(default = "default_true")]
    release: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
struct RebuildOutput {
    success: bool,
    components_built: Vec<String>,
    duration_secs: f64,
    output: String,
}

pub fn execute_rebuild(input: &[u8]) -> Result<Vec<u8>> {
    let input: RebuildInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let start = std::time::Instant::now();

    let mut args = vec!["build", "--workspace"];
    if input.release {
        args.push("--release");
    }

    // Build specific components if requested
    let component_args: Vec<String>;
    if !input.components.is_empty() {
        component_args = input
            .components
            .iter()
            .flat_map(|c| vec!["-p".to_string(), c.clone()])
            .collect();
        args = vec!["build"];
        if input.release {
            args.push("--release");
        }
    } else {
        component_args = Vec::new();
    }

    let mut cmd = Command::new("cargo");
    cmd.args(&args).current_dir(&input.source_path);

    if !component_args.is_empty() {
        let arg_refs: Vec<&str> = component_args.iter().map(|s| s.as_str()).collect();
        cmd.args(&arg_refs);
    }

    let output = cmd.output().context("Failed to execute cargo build")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        anyhow::bail!("Build failed:\n{stderr}");
    }

    let duration_secs = start.elapsed().as_secs_f64();

    let components_built = if input.components.is_empty() {
        vec![
            "aios-orchestrator".to_string(),
            "aios-tools".to_string(),
            "aios-memory".to_string(),
            "aios-runtime".to_string(),
            "aios-api-gateway".to_string(),
            "aios-init".to_string(),
        ]
    } else {
        input.components
    };

    let result = RebuildOutput {
        success: true,
        components_built,
        duration_secs,
        output: format!("{stdout}\n{stderr}").trim().to_string(),
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
