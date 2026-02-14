//! pkg.search — Search for available packages

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Deserialize)]
struct Input {
    query: String,
}

#[derive(Serialize)]
struct Output {
    packages: Vec<PackageEntry>,
}

#[derive(Serialize)]
struct PackageEntry {
    name: String,
    version: String,
    description: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let packages = if cfg!(target_os = "macos") {
        search_brew(&input.query)?
    } else {
        search_linux(&input.query)?
    };

    let result = Output { packages };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn search_brew(query: &str) -> Result<Vec<PackageEntry>> {
    let output = Command::new("brew")
        .args(["search", query])
        .output()
        .context("Failed to execute brew search")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in stdout.lines() {
        let name = line.trim().to_string();
        if name.is_empty() || name.starts_with("==>") {
            continue;
        }

        // Get version and description for each package (limit to first 20 results)
        if packages.len() >= 20 {
            break;
        }

        let (version, description) = get_brew_info(&name);

        packages.push(PackageEntry {
            name,
            version,
            description,
        });
    }

    Ok(packages)
}

fn get_brew_info(name: &str) -> (String, String) {
    let output = Command::new("brew")
        .args(["info", "--json=v2", name])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                // Try formulae first
                if let Some(formulae) = v.get("formulae").and_then(|f| f.as_array()) {
                    if let Some(first) = formulae.first() {
                        let version = first
                            .get("versions")
                            .and_then(|v| v.get("stable"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let desc = first
                            .get("desc")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string();
                        return (version, desc);
                    }
                }
                // Try casks
                if let Some(casks) = v.get("casks").and_then(|c| c.as_array()) {
                    if let Some(first) = casks.first() {
                        let version = first
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let desc = first
                            .get("desc")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string();
                        return (version, desc);
                    }
                }
            }
            ("unknown".to_string(), String::new())
        }
        _ => ("unknown".to_string(), String::new()),
    }
}

fn search_linux(query: &str) -> Result<Vec<PackageEntry>> {
    if std::path::Path::new("/usr/bin/apt-cache").exists() {
        search_apt(query)
    } else if std::path::Path::new("/usr/bin/dnf").exists() {
        search_dnf(query)
    } else if std::path::Path::new("/usr/bin/pacman").exists() {
        search_pacman(query)
    } else {
        Err(anyhow::anyhow!("No supported package manager found"))
    }
}

fn search_apt(query: &str) -> Result<Vec<PackageEntry>> {
    let output = Command::new("apt-cache")
        .args(["search", query])
        .output()
        .context("Failed to execute apt-cache search")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in stdout.lines().take(50) {
        // Format: "package-name - Description text"
        let parts: Vec<&str> = line.splitn(2, " - ").collect();
        if parts.len() == 2 {
            let name = parts[0].trim().to_string();
            let description = parts[1].trim().to_string();

            // Get version from apt-cache policy
            let version = get_apt_version(&name);

            packages.push(PackageEntry {
                name,
                version,
                description,
            });
        }
    }

    Ok(packages)
}

fn get_apt_version(name: &str) -> String {
    let output = Command::new("apt-cache").args(["policy", name]).output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if let Some(ver) = trimmed.strip_prefix("Candidate: ") {
                    return ver.trim().to_string();
                }
            }
            "unknown".to_string()
        }
        Err(_) => "unknown".to_string(),
    }
}

fn search_dnf(query: &str) -> Result<Vec<PackageEntry>> {
    let output = Command::new("dnf")
        .args(["search", "--quiet", query])
        .output()
        .context("Failed to execute dnf search")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in stdout.lines().take(50) {
        // Format: "name.arch : Description"
        let parts: Vec<&str> = line.splitn(2, " : ").collect();
        if parts.len() == 2 {
            let name_arch = parts[0].trim();
            let name = name_arch.split('.').next().unwrap_or(name_arch).to_string();
            let description = parts[1].trim().to_string();

            packages.push(PackageEntry {
                name,
                version: "unknown".to_string(),
                description,
            });
        }
    }

    Ok(packages)
}

fn search_pacman(query: &str) -> Result<Vec<PackageEntry>> {
    let output = Command::new("pacman")
        .args(["-Ss", query])
        .output()
        .context("Failed to execute pacman -Ss")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();
    let lines: Vec<&str> = stdout.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Format: "repo/name version [installed]"
        //         "    Description text"
        if !line.starts_with(' ') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let repo_name = parts[0];
                let name = repo_name.split('/').last().unwrap_or(repo_name).to_string();
                let version = parts[1].to_string();

                let description = if i + 1 < lines.len() && lines[i + 1].starts_with(' ') {
                    lines[i + 1].trim().to_string()
                } else {
                    String::new()
                };

                packages.push(PackageEntry {
                    name,
                    version,
                    description,
                });
            }
        }
        i += 1;
    }

    Ok(packages)
}
