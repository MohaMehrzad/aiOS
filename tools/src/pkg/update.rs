//! pkg.update — Update all installed packages

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    updated: u32,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let updated = if cfg!(target_os = "macos") {
        update_brew()?
    } else {
        update_linux()?
    };

    let result = Output { updated };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn update_brew() -> Result<u32> {
    // First, update the formula index
    let _update = Command::new("brew")
        .arg("update")
        .output()
        .context("Failed to execute brew update")?;

    // Check what's outdated before upgrading
    let outdated = Command::new("brew")
        .args(["outdated", "--json=v2"])
        .output()
        .context("Failed to check outdated brew packages")?;

    let outdated_count = if outdated.status.success() {
        let stdout = String::from_utf8_lossy(&outdated.stdout);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
            let formulae_count = v
                .get("formulae")
                .and_then(|f| f.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let cask_count = v
                .get("casks")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            (formulae_count + cask_count) as u32
        } else {
            0
        }
    } else {
        0
    };

    if outdated_count > 0 {
        // Perform the upgrade
        let upgrade = Command::new("brew")
            .arg("upgrade")
            .output()
            .context("Failed to execute brew upgrade")?;

        if !upgrade.status.success() {
            let stderr = String::from_utf8_lossy(&upgrade.stderr);
            anyhow::bail!("brew upgrade failed: {}", stderr.trim());
        }
    }

    Ok(outdated_count)
}

fn update_linux() -> Result<u32> {
    if std::path::Path::new("/usr/bin/apt-get").exists() {
        update_apt()
    } else if std::path::Path::new("/usr/bin/dnf").exists() {
        update_dnf()
    } else if std::path::Path::new("/usr/bin/pacman").exists() {
        update_pacman()
    } else {
        Err(anyhow::anyhow!("No supported package manager found"))
    }
}

fn update_apt() -> Result<u32> {
    // Update package lists
    let update = Command::new("apt-get")
        .args(["update", "-qq"])
        .env("DEBIAN_FRONTEND", "noninteractive")
        .output()
        .context("Failed to execute apt-get update")?;

    if !update.status.success() {
        let stderr = String::from_utf8_lossy(&update.stderr);
        anyhow::bail!("apt-get update failed: {}", stderr.trim());
    }

    // Check how many packages can be upgraded
    let check = Command::new("apt-get")
        .args(["upgrade", "--dry-run", "-qq"])
        .env("DEBIAN_FRONTEND", "noninteractive")
        .output()
        .context("Failed to check upgradable packages")?;

    let stdout = String::from_utf8_lossy(&check.stdout);
    let count = stdout.lines().filter(|l| l.starts_with("Inst ")).count() as u32;

    if count > 0 {
        let upgrade = Command::new("apt-get")
            .args(["upgrade", "-y", "-qq"])
            .env("DEBIAN_FRONTEND", "noninteractive")
            .output()
            .context("Failed to execute apt-get upgrade")?;

        if !upgrade.status.success() {
            let stderr = String::from_utf8_lossy(&upgrade.stderr);
            anyhow::bail!("apt-get upgrade failed: {}", stderr.trim());
        }
    }

    Ok(count)
}

fn update_dnf() -> Result<u32> {
    let check = Command::new("dnf")
        .args(["check-update", "--quiet"])
        .output()
        .context("Failed to check dnf updates")?;

    // dnf check-update returns exit code 100 if updates are available
    let stdout = String::from_utf8_lossy(&check.stdout);
    let count = stdout
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with(' '))
        .count() as u32;

    if count > 0 {
        let update = Command::new("dnf")
            .args(["update", "-y", "--quiet"])
            .output()
            .context("Failed to execute dnf update")?;

        if !update.status.success() {
            let stderr = String::from_utf8_lossy(&update.stderr);
            anyhow::bail!("dnf update failed: {}", stderr.trim());
        }
    }

    Ok(count)
}

fn update_pacman() -> Result<u32> {
    // Check for updates
    let check = Command::new("pacman")
        .args(["-Qu"])
        .output()
        .context("Failed to check pacman updates")?;

    let stdout = String::from_utf8_lossy(&check.stdout);
    let count = stdout.lines().filter(|l| !l.is_empty()).count() as u32;

    if count > 0 {
        let update = Command::new("pacman")
            .args(["-Syu", "--noconfirm"])
            .output()
            .context("Failed to execute pacman -Syu")?;

        if !update.status.success() {
            let stderr = String::from_utf8_lossy(&update.stderr);
            anyhow::bail!("pacman -Syu failed: {}", stderr.trim());
        }
    }

    Ok(count)
}
