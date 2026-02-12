//! pkg.install — Install a package

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    name: String,
}

#[derive(Serialize)]
struct Output {
    installed: bool,
    version: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let (installed, version) = if cfg!(target_os = "macos") {
        install_brew(&input.name)?
    } else {
        install_linux(&input.name)?
    };

    let result = Output { installed, version };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn install_brew(name: &str) -> Result<(bool, String)> {
    let output = Command::new("brew")
        .args(["install", name])
        .output()
        .context("Failed to execute brew install. Ensure Homebrew is installed.")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("brew install {} failed: {}", name, stderr.trim());
    }

    // Get the installed version
    let version_output = Command::new("brew")
        .args(["info", "--json=v2", name])
        .output()
        .context("Failed to get package info from brew")?;

    let version = if version_output.status.success() {
        let stdout = String::from_utf8_lossy(&version_output.stdout);
        parse_brew_version(&stdout).unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    };

    Ok((true, version))
}

fn parse_brew_version(json_output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json_output).ok()?;

    // Try formulae first, then casks
    if let Some(formulae) = v.get("formulae").and_then(|f| f.as_array()) {
        if let Some(first) = formulae.first() {
            if let Some(versions) = first.get("versions") {
                if let Some(stable) = versions.get("stable").and_then(|s| s.as_str()) {
                    return Some(stable.to_string());
                }
            }
        }
    }

    if let Some(casks) = v.get("casks").and_then(|c| c.as_array()) {
        if let Some(first) = casks.first() {
            if let Some(version) = first.get("version").and_then(|v| v.as_str()) {
                return Some(version.to_string());
            }
        }
    }

    None
}

fn install_linux(name: &str) -> Result<(bool, String)> {
    // Detect package manager
    let (pm, install_args) = detect_package_manager()?;

    let mut cmd = Command::new(&pm);
    for arg in &install_args {
        cmd.arg(arg);
    }
    cmd.arg(name);
    cmd.env("DEBIAN_FRONTEND", "noninteractive");

    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute {} install {}", pm, name))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} install {} failed: {}", pm, name, stderr.trim());
    }

    // Get version
    let version = get_linux_package_version(&pm, name);

    Ok((true, version))
}

fn detect_package_manager() -> Result<(String, Vec<String>)> {
    if std::path::Path::new("/usr/bin/apt-get").exists() {
        Ok(("apt-get".to_string(), vec!["install".to_string(), "-y".to_string()]))
    } else if std::path::Path::new("/usr/bin/dnf").exists() {
        Ok(("dnf".to_string(), vec!["install".to_string(), "-y".to_string()]))
    } else if std::path::Path::new("/usr/bin/yum").exists() {
        Ok(("yum".to_string(), vec!["install".to_string(), "-y".to_string()]))
    } else if std::path::Path::new("/usr/bin/pacman").exists() {
        Ok(("pacman".to_string(), vec!["-S".to_string(), "--noconfirm".to_string()]))
    } else {
        Err(anyhow::anyhow!("No supported package manager found"))
    }
}

fn get_linux_package_version(pm: &str, name: &str) -> String {
    let output = match pm {
        "apt-get" => Command::new("dpkg")
            .args(["-s", name])
            .output(),
        "dnf" | "yum" => Command::new("rpm")
            .args(["-q", "--queryformat", "%{VERSION}", name])
            .output(),
        "pacman" => Command::new("pacman")
            .args(["-Q", name])
            .output(),
        _ => return "unknown".to_string(),
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if pm == "apt-get" {
                // Parse "Version: x.y.z" from dpkg -s output
                for line in stdout.lines() {
                    if let Some(ver) = line.strip_prefix("Version: ") {
                        return ver.trim().to_string();
                    }
                }
            } else if pm == "pacman" {
                // Format: "name version"
                if let Some(ver) = stdout.split_whitespace().nth(1) {
                    return ver.to_string();
                }
            } else {
                return stdout.trim().to_string();
            }
            "unknown".to_string()
        }
        Err(_) => "unknown".to_string(),
    }
}
