//! Git operations — all git tool implementations via the git CLI

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

// ── git.init ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct InitInput {
    path: String,
    #[serde(default)]
    bare: bool,
}

#[derive(Serialize)]
struct InitOutput {
    success: bool,
    path: String,
}

pub fn execute_init(input: &[u8]) -> Result<Vec<u8>> {
    let input: InitInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    std::fs::create_dir_all(&input.path)
        .with_context(|| format!("Failed to create directory: {}", input.path))?;

    let mut args = vec!["init"];
    if input.bare {
        args.push("--bare");
    }
    args.push(&input.path);

    let output = Command::new("git")
        .args(&args)
        .output()
        .context("Failed to execute git init")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git init failed: {stderr}");
    }

    serde_json::to_vec(&InitOutput {
        success: true,
        path: input.path,
    })
    .context("Failed to serialize output")
}

// ── git.clone ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CloneInput {
    url: String,
    destination: String,
    #[serde(default)]
    branch: String,
    #[serde(default = "default_depth")]
    depth: u32,
}

fn default_depth() -> u32 {
    0
}

#[derive(Serialize)]
struct CloneOutput {
    success: bool,
    url: String,
    destination: String,
}

pub fn execute_clone(input: &[u8]) -> Result<Vec<u8>> {
    let input: CloneInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut args = vec!["clone".to_string()];

    if !input.branch.is_empty() {
        args.push("-b".to_string());
        args.push(input.branch);
    }

    if input.depth > 0 {
        args.push("--depth".to_string());
        args.push(input.depth.to_string());
    }

    args.push(input.url.clone());
    args.push(input.destination.clone());

    let output = Command::new("git")
        .args(&args)
        .output()
        .context("Failed to execute git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {stderr}");
    }

    serde_json::to_vec(&CloneOutput {
        success: true,
        url: input.url,
        destination: input.destination,
    })
    .context("Failed to serialize output")
}

// ── git.add ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AddInput {
    repo_path: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    all: bool,
}

#[derive(Serialize)]
struct AddOutput {
    success: bool,
    files_staged: Vec<String>,
}

pub fn execute_add(input: &[u8]) -> Result<Vec<u8>> {
    let input: AddInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut args = vec!["add"];

    if input.all || input.files.is_empty() {
        args.push("-A");
    } else {
        let file_refs: Vec<&str> = input.files.iter().map(|s| s.as_str()).collect();
        args.extend(&file_refs);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to execute git add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git add failed: {stderr}");
    }

    serde_json::to_vec(&AddOutput {
        success: true,
        files_staged: if input.all {
            vec!["all".to_string()]
        } else {
            input.files
        },
    })
    .context("Failed to serialize output")
}

// ── git.commit ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CommitInput {
    repo_path: String,
    message: String,
    #[serde(default)]
    author: String,
}

#[derive(Serialize)]
struct CommitOutput {
    success: bool,
    commit_hash: String,
    message: String,
}

pub fn execute_commit(input: &[u8]) -> Result<Vec<u8>> {
    let input: CommitInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut args = vec!["commit".to_string(), "-m".to_string(), input.message.clone()];

    if !input.author.is_empty() {
        args.push("--author".to_string());
        args.push(input.author);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to execute git commit")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git commit failed: {stderr}");
    }

    // Get the commit hash
    let hash_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to get commit hash")?;

    let commit_hash = String::from_utf8_lossy(&hash_output.stdout)
        .trim()
        .to_string();

    serde_json::to_vec(&CommitOutput {
        success: true,
        commit_hash,
        message: input.message,
    })
    .context("Failed to serialize output")
}

// ── git.push ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PushInput {
    repo_path: String,
    #[serde(default = "default_remote")]
    remote: String,
    #[serde(default)]
    branch: String,
}

fn default_remote() -> String {
    "origin".to_string()
}

#[derive(Serialize)]
struct PushOutput {
    success: bool,
    remote: String,
    branch: String,
}

pub fn execute_push(input: &[u8]) -> Result<Vec<u8>> {
    let input: PushInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut args = vec!["push", &input.remote];
    if !input.branch.is_empty() {
        args.push(&input.branch);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to execute git push")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git push failed: {stderr}");
    }

    serde_json::to_vec(&PushOutput {
        success: true,
        remote: input.remote,
        branch: input.branch,
    })
    .context("Failed to serialize output")
}

// ── git.pull ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PullInput {
    repo_path: String,
    #[serde(default = "default_remote")]
    remote: String,
    #[serde(default)]
    branch: String,
}

#[derive(Serialize)]
struct PullOutput {
    success: bool,
    remote: String,
    output: String,
}

pub fn execute_pull(input: &[u8]) -> Result<Vec<u8>> {
    let input: PullInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut args = vec!["pull", &input.remote];
    if !input.branch.is_empty() {
        args.push(&input.branch);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to execute git pull")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git pull failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    serde_json::to_vec(&PullOutput {
        success: true,
        remote: input.remote,
        output: stdout,
    })
    .context("Failed to serialize output")
}

// ── git.branch ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct BranchInput {
    repo_path: String,
    /// "create", "list", "switch", or "delete"
    #[serde(default = "default_action_list")]
    action: String,
    #[serde(default)]
    name: String,
}

fn default_action_list() -> String {
    "list".to_string()
}

#[derive(Serialize)]
struct BranchOutput {
    success: bool,
    action: String,
    branches: Vec<String>,
    current: String,
}

pub fn execute_branch(input: &[u8]) -> Result<Vec<u8>> {
    let input: BranchInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    match input.action.as_str() {
        "create" => {
            let output = Command::new("git")
                .args(["branch", &input.name])
                .current_dir(&input.repo_path)
                .output()
                .context("Failed to create branch")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git branch create failed: {stderr}");
            }
        }
        "switch" | "checkout" => {
            let output = Command::new("git")
                .args(["checkout", &input.name])
                .current_dir(&input.repo_path)
                .output()
                .context("Failed to switch branch")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git checkout failed: {stderr}");
            }
        }
        "delete" => {
            let output = Command::new("git")
                .args(["branch", "-d", &input.name])
                .current_dir(&input.repo_path)
                .output()
                .context("Failed to delete branch")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git branch delete failed: {stderr}");
            }
        }
        _ => {} // "list" falls through
    }

    // Always list branches and current
    let list_output = Command::new("git")
        .args(["branch", "--list"])
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to list branches")?;

    let branch_text = String::from_utf8_lossy(&list_output.stdout).to_string();
    let mut current = String::new();
    let branches: Vec<String> = branch_text
        .lines()
        .map(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with("* ") {
                current = trimmed[2..].to_string();
                current.clone()
            } else {
                trimmed.to_string()
            }
        })
        .filter(|b| !b.is_empty())
        .collect();

    serde_json::to_vec(&BranchOutput {
        success: true,
        action: input.action,
        branches,
        current,
    })
    .context("Failed to serialize output")
}

// ── git.status ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct StatusInput {
    repo_path: String,
}

#[derive(Serialize)]
struct StatusOutput {
    clean: bool,
    branch: String,
    staged: Vec<String>,
    modified: Vec<String>,
    untracked: Vec<String>,
}

pub fn execute_status(input: &[u8]) -> Result<Vec<u8>> {
    let input: StatusInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-b"])
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to execute git status")?;

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut branch = String::new();
    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();

    for line in text.lines() {
        if line.starts_with("## ") {
            branch = line[3..]
                .split("...")
                .next()
                .unwrap_or("")
                .to_string();
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let index = line.chars().next().unwrap_or(' ');
        let worktree = line.chars().nth(1).unwrap_or(' ');
        let filename = line[3..].to_string();

        if index == '?' && worktree == '?' {
            untracked.push(filename);
        } else {
            if index != ' ' && index != '?' {
                staged.push(filename.clone());
            }
            if worktree != ' ' && worktree != '?' {
                modified.push(filename);
            }
        }
    }

    serde_json::to_vec(&StatusOutput {
        clean: staged.is_empty() && modified.is_empty() && untracked.is_empty(),
        branch,
        staged,
        modified,
        untracked,
    })
    .context("Failed to serialize output")
}

// ── git.log ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LogInput {
    repo_path: String,
    #[serde(default = "default_count")]
    count: u32,
}

fn default_count() -> u32 {
    10
}

#[derive(Serialize)]
struct LogEntry {
    hash: String,
    author: String,
    date: String,
    message: String,
}

#[derive(Serialize)]
struct LogOutput {
    entries: Vec<LogEntry>,
}

pub fn execute_log(input: &[u8]) -> Result<Vec<u8>> {
    let input: LogInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let output = Command::new("git")
        .args([
            "log",
            &format!("-{}", input.count),
            "--pretty=format:%H%n%an%n%aI%n%s%n---",
        ])
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to execute git log")?;

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut entries = Vec::new();

    for block in text.split("---") {
        let lines: Vec<&str> = block.trim().lines().collect();
        if lines.len() >= 4 {
            entries.push(LogEntry {
                hash: lines[0].to_string(),
                author: lines[1].to_string(),
                date: lines[2].to_string(),
                message: lines[3..].join(" "),
            });
        }
    }

    serde_json::to_vec(&LogOutput { entries }).context("Failed to serialize output")
}

// ── git.diff ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DiffInput {
    repo_path: String,
    #[serde(default)]
    staged: bool,
    #[serde(default)]
    commit: String,
}

#[derive(Serialize)]
struct DiffOutput {
    diff: String,
    files_changed: usize,
}

pub fn execute_diff(input: &[u8]) -> Result<Vec<u8>> {
    let input: DiffInput = serde_json::from_slice(input).context("Invalid JSON input")?;

    let mut args = vec!["diff".to_string()];
    if input.staged {
        args.push("--cached".to_string());
    }
    if !input.commit.is_empty() {
        args.push(input.commit);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(&input.repo_path)
        .output()
        .context("Failed to execute git diff")?;

    let diff_text = String::from_utf8_lossy(&output.stdout).to_string();
    let files_changed = diff_text
        .lines()
        .filter(|l| l.starts_with("diff --git"))
        .count();

    // Truncate if too large
    let max_len = 500_000;
    let diff = if diff_text.len() > max_len {
        format!(
            "{}... [truncated, {} total bytes]",
            &diff_text[..max_len],
            diff_text.len()
        )
    } else {
        diff_text
    };

    serde_json::to_vec(&DiffOutput {
        diff,
        files_changed,
    })
    .context("Failed to serialize output")
}
