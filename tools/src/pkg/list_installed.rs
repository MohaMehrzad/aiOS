//! pkg.list_installed — List all installed packages

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {}

#[derive(Serialize)]
struct Output {
    packages: Vec<PackageEntry>,
}

#[derive(Serialize)]
struct PackageEntry {
    name: String,
    version: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let _input: Input = if input.is_empty() {
        Input {}
    } else {
        serde_json::from_slice(input).context("Invalid JSON input")?
    };

    let packages = if cfg!(target_os = "macos") {
        list_brew()?
    } else {
        list_linux()?
    };

    let result = Output { packages };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn list_brew() -> Result<Vec<PackageEntry>> {
    let output = Command::new("brew")
        .args(["list", "--versions"])
        .output()
        .context("Failed to execute brew list")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].to_string();
        // brew list --versions can show multiple versions; take the latest (last)
        let version = parts.last().unwrap_or(&"unknown").to_string();

        packages.push(PackageEntry { name, version });
    }

    Ok(packages)
}

fn list_linux() -> Result<Vec<PackageEntry>> {
    if std::path::Path::new("/usr/bin/dpkg").exists() {
        list_dpkg()
    } else if std::path::Path::new("/usr/bin/rpm").exists() {
        list_rpm()
    } else if std::path::Path::new("/usr/bin/pacman").exists() {
        list_pacman()
    } else {
        Err(anyhow::anyhow!("No supported package manager found"))
    }
}

fn list_dpkg() -> Result<Vec<PackageEntry>> {
    let output = Command::new("dpkg-query")
        .args(["-W", "-f", "${Package}\t${Version}\n"])
        .output()
        .context("Failed to execute dpkg-query")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            packages.push(PackageEntry {
                name: parts[0].to_string(),
                version: parts[1].to_string(),
            });
        }
    }

    Ok(packages)
}

fn list_rpm() -> Result<Vec<PackageEntry>> {
    let output = Command::new("rpm")
        .args(["-qa", "--queryformat", "%{NAME}\t%{VERSION}-%{RELEASE}\n"])
        .output()
        .context("Failed to execute rpm -qa")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            packages.push(PackageEntry {
                name: parts[0].to_string(),
                version: parts[1].to_string(),
            });
        }
    }

    Ok(packages)
}

fn list_pacman() -> Result<Vec<PackageEntry>> {
    let output = Command::new("pacman")
        .args(["-Q"])
        .output()
        .context("Failed to execute pacman -Q")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            packages.push(PackageEntry {
                name: parts[0].to_string(),
                version: parts[1].to_string(),
            });
        }
    }

    Ok(packages)
}
