//! pkg.remove — Remove an installed package

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    name: String,
}

#[derive(Serialize)]
struct Output {
    removed: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let removed = if cfg!(target_os = "macos") {
        remove_brew(&input.name)?
    } else {
        remove_linux(&input.name)?
    };

    let result = Output { removed };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn remove_brew(name: &str) -> Result<bool> {
    let output = Command::new("brew")
        .args(["uninstall", name])
        .output()
        .context("Failed to execute brew uninstall")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("brew uninstall {} failed: {}", name, stderr.trim());
    }

    Ok(true)
}

fn remove_linux(name: &str) -> Result<bool> {
    let (pm, remove_args) = detect_remove_command()?;

    let mut cmd = Command::new(&pm);
    for arg in &remove_args {
        cmd.arg(arg);
    }
    cmd.arg(name);
    cmd.env("DEBIAN_FRONTEND", "noninteractive");

    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute {} remove {}", pm, name))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} remove {} failed: {}", pm, name, stderr.trim());
    }

    Ok(true)
}

fn detect_remove_command() -> Result<(String, Vec<String>)> {
    if std::path::Path::new("/usr/bin/apt-get").exists() {
        Ok((
            "apt-get".to_string(),
            vec!["remove".to_string(), "-y".to_string()],
        ))
    } else if std::path::Path::new("/usr/bin/dnf").exists() {
        Ok((
            "dnf".to_string(),
            vec!["remove".to_string(), "-y".to_string()],
        ))
    } else if std::path::Path::new("/usr/bin/yum").exists() {
        Ok((
            "yum".to_string(),
            vec!["remove".to_string(), "-y".to_string()],
        ))
    } else if std::path::Path::new("/usr/bin/pacman").exists() {
        Ok((
            "pacman".to_string(),
            vec!["-R".to_string(), "--noconfirm".to_string()],
        ))
    } else {
        Err(anyhow::anyhow!("No supported package manager found"))
    }
}
