//! Plugin creation and dependency installation
//!
//! `plugin.create` — writes a Python plugin script and metadata to disk.
//! `plugin.install_deps` — installs pip packages for a plugin.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::info;

use super::{PluginMetadata, PLUGIN_DIR};

/// Input for plugin.create
#[derive(Debug, Deserialize)]
struct CreateInput {
    /// Plugin name (alphanumeric + underscore only)
    name: String,
    /// Human-readable description of what the plugin does
    description: String,
    /// Python code defining `def main(input_data: dict) -> dict`
    code: String,
    /// Capabilities this plugin requires (e.g., ["net_read"])
    #[serde(default)]
    capabilities: Vec<String>,
    /// Pip package dependencies (e.g., ["requests", "beautifulsoup4"])
    #[serde(default)]
    dependencies: Vec<String>,
}

/// Output for plugin.create
#[derive(Debug, Serialize)]
struct CreateOutput {
    success: bool,
    tool_name: String,
    script_path: String,
    metadata_path: String,
    dependencies_installed: bool,
}

/// Execute plugin.create — write script + metadata to PLUGIN_DIR
pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: CreateInput =
        serde_json::from_slice(input).context("Invalid plugin.create input JSON")?;

    // Validate name: alphanumeric + underscore only
    if req.name.is_empty() || !req.name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        bail!(
            "Invalid plugin name '{}': must be non-empty, alphanumeric + underscore only",
            req.name
        );
    }

    // Ensure plugin directory exists
    std::fs::create_dir_all(PLUGIN_DIR)
        .with_context(|| format!("Failed to create plugin directory: {PLUGIN_DIR}"))?;

    let tool_name = format!("plugin.{}", req.name);
    let script_path = format!("{}/{}.py", PLUGIN_DIR, req.name);
    let metadata_path = format!("{}/{}.meta.json", PLUGIN_DIR, req.name);

    // Wrap the user code in a standard harness
    let wrapped_code = wrap_plugin_code(&req.name, &req.code);

    // Write the Python script
    std::fs::write(&script_path, &wrapped_code)
        .with_context(|| format!("Failed to write plugin script to {script_path}"))?;

    // Make script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&script_path, perms).ok();
    }

    // Build metadata
    let now = chrono::Utc::now().to_rfc3339();
    let metadata = PluginMetadata {
        tool_name: tool_name.clone(),
        description: req.description,
        capabilities: req.capabilities,
        dependencies: req.dependencies.clone(),
        author: "aiOS-autonomy".to_string(),
        created_at: now,
        timeout_ms: 30000,
    };

    // Write metadata
    let meta_json = serde_json::to_string_pretty(&metadata)
        .context("Failed to serialize plugin metadata")?;
    std::fs::write(&metadata_path, &meta_json)
        .with_context(|| format!("Failed to write plugin metadata to {metadata_path}"))?;

    // Install dependencies if any
    let deps_installed = if !req.dependencies.is_empty() {
        match install_pip_deps(&req.dependencies) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!("Failed to install dependencies for plugin {}: {e}", req.name);
                false
            }
        }
    } else {
        true
    };

    info!(
        "Created plugin '{}' at {} (deps_installed: {})",
        tool_name, script_path, deps_installed
    );

    let output = CreateOutput {
        success: true,
        tool_name,
        script_path,
        metadata_path,
        dependencies_installed: deps_installed,
    };

    serde_json::to_vec(&output).context("Failed to serialize plugin.create output")
}

/// Wrap AI-written code in a stdin/stdout JSON harness.
/// The AI writes `def main(input_data: dict) -> dict`, and the wrapper
/// handles reading JSON from stdin and writing JSON to stdout.
fn wrap_plugin_code(name: &str, user_code: &str) -> String {
    format!(
        r#"#!/usr/bin/env python3
"""aiOS Plugin: {name}

Auto-generated plugin wrapper. The main() function below was written by AI.
Input is read as JSON from stdin, output is written as JSON to stdout.
"""

import sys
import json
import traceback

# --- AI-generated plugin code ---

{user_code}

# --- End AI-generated code ---

if __name__ == "__main__":
    try:
        input_data = json.loads(sys.stdin.read()) if not sys.stdin.isatty() else {{}}
        result = main(input_data)
        if not isinstance(result, dict):
            result = {{"result": result}}
        json.dump(result, sys.stdout)
    except Exception as e:
        json.dump({{"error": str(e), "traceback": traceback.format_exc()}}, sys.stdout)
        sys.exit(1)
"#
    )
}

/// Input for plugin.install_deps
#[derive(Debug, Deserialize)]
struct InstallDepsInput {
    /// Plugin name to install dependencies for
    #[serde(default)]
    name: String,
    /// Additional packages to install (beyond those in metadata)
    #[serde(default)]
    packages: Vec<String>,
}

/// Output for plugin.install_deps
#[derive(Debug, Serialize)]
struct InstallDepsOutput {
    success: bool,
    packages_installed: Vec<String>,
    error: String,
}

/// Execute plugin.install_deps — install pip packages
pub fn execute_install_deps(input: &[u8]) -> Result<Vec<u8>> {
    let req: InstallDepsInput =
        serde_json::from_slice(input).context("Invalid plugin.install_deps input JSON")?;

    let mut all_packages = req.packages.clone();

    // If a plugin name is given, load its dependencies from metadata
    if !req.name.is_empty() {
        let metadata_path = format!("{}/{}.meta.json", PLUGIN_DIR, req.name);
        if Path::new(&metadata_path).exists() {
            if let Ok(contents) = std::fs::read_to_string(&metadata_path) {
                if let Ok(meta) = serde_json::from_str::<PluginMetadata>(&contents) {
                    all_packages.extend(meta.dependencies);
                }
            }
        }
    }

    // Deduplicate
    all_packages.sort();
    all_packages.dedup();

    if all_packages.is_empty() {
        let output = InstallDepsOutput {
            success: true,
            packages_installed: vec![],
            error: String::new(),
        };
        return serde_json::to_vec(&output).context("Failed to serialize output");
    }

    match install_pip_deps(&all_packages) {
        Ok(_) => {
            let output = InstallDepsOutput {
                success: true,
                packages_installed: all_packages,
                error: String::new(),
            };
            serde_json::to_vec(&output).context("Failed to serialize output")
        }
        Err(e) => {
            let output = InstallDepsOutput {
                success: false,
                packages_installed: vec![],
                error: e.to_string(),
            };
            serde_json::to_vec(&output).context("Failed to serialize output")
        }
    }
}

/// Install pip packages using pip3
fn install_pip_deps(packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    // Validate package names — only allow alphanumeric, dash, underscore, dot, brackets, comparison ops
    for pkg in packages {
        if !pkg.chars().all(|c| {
            c.is_alphanumeric() || "-_.[]>=<!, ".contains(c)
        }) {
            bail!("Invalid package name: {pkg}");
        }
    }

    info!("Installing pip dependencies: {:?}", packages);

    let mut args = vec![
        "install".to_string(),
        "--user".to_string(),
        "--quiet".to_string(),
    ];
    args.extend(packages.iter().cloned());

    let output = std::process::Command::new("pip3")
        .args(&args)
        .output()
        .context("Failed to run pip3 install")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pip3 install failed: {stderr}");
    }

    info!("Successfully installed: {:?}", packages);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_plugin_code() {
        let code = "def main(input_data):\n    return {\"result\": \"hello\"}";
        let wrapped = wrap_plugin_code("test_plugin", code);
        assert!(wrapped.contains("def main(input_data)"));
        assert!(wrapped.contains("json.loads(sys.stdin.read())"));
        assert!(wrapped.contains("json.dump(result, sys.stdout)"));
        assert!(wrapped.contains("aiOS Plugin: test_plugin"));
    }

    #[test]
    fn test_create_invalid_name() {
        let input = serde_json::to_vec(&serde_json::json!({
            "name": "bad name!",
            "description": "test",
            "code": "def main(d): return {}",
        }))
        .unwrap();

        let result = execute(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_empty_name() {
        let input = serde_json::to_vec(&serde_json::json!({
            "name": "",
            "description": "test",
            "code": "def main(d): return {}",
        }))
        .unwrap();

        let result = execute(&input);
        assert!(result.is_err());
    }
}
