//! Plugin management — list and delete plugin tools

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::{PluginMetadata, PLUGIN_DIR};

/// Output entry for plugin.list
#[derive(Debug, Serialize, Deserialize)]
struct PluginListEntry {
    tool_name: String,
    description: String,
    script_path: String,
    dependencies: Vec<String>,
    created_at: String,
}

/// Output for plugin.list
#[derive(Debug, Serialize, Deserialize)]
struct ListOutput {
    plugins: Vec<PluginListEntry>,
    count: usize,
}

/// Execute plugin.list — scan PLUGIN_DIR for installed plugins
pub fn execute_list(input: &[u8]) -> Result<Vec<u8>> {
    // Input is ignored (no filtering for now), but accept it gracefully
    let _ = input;

    let plugin_dir = std::path::Path::new(PLUGIN_DIR);
    if !plugin_dir.exists() {
        let output = ListOutput {
            plugins: vec![],
            count: 0,
        };
        return serde_json::to_vec(&output).context("Failed to serialize plugin.list output");
    }

    let entries = std::fs::read_dir(plugin_dir)
        .with_context(|| format!("Failed to read plugin directory: {PLUGIN_DIR}"))?;

    let mut plugins = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json")
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or(false, |n| n.ends_with(".meta.json"))
        {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(meta) = serde_json::from_str::<PluginMetadata>(&contents) {
                    // Derive script path from tool name
                    let short_name = meta
                        .tool_name
                        .strip_prefix("plugin.")
                        .unwrap_or(&meta.tool_name);
                    let script_path = format!("{}/{}.py", PLUGIN_DIR, short_name);

                    plugins.push(PluginListEntry {
                        tool_name: meta.tool_name,
                        description: meta.description,
                        script_path,
                        dependencies: meta.dependencies,
                        created_at: meta.created_at,
                    });
                }
            }
        }
    }

    let count = plugins.len();
    let output = ListOutput { plugins, count };

    serde_json::to_vec(&output).context("Failed to serialize plugin.list output")
}

/// Input for plugin.delete
#[derive(Debug, Deserialize)]
struct DeleteInput {
    /// Plugin name (without "plugin." prefix)
    name: String,
}

/// Output for plugin.delete
#[derive(Debug, Serialize)]
struct DeleteOutput {
    success: bool,
    deleted_files: Vec<String>,
}

/// Execute plugin.delete — remove a plugin's script and metadata files
pub fn execute_delete(input: &[u8]) -> Result<Vec<u8>> {
    let req: DeleteInput =
        serde_json::from_slice(input).context("Invalid plugin.delete input JSON")?;

    if req.name.is_empty() || !req.name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        bail!(
            "Invalid plugin name '{}': must be non-empty, alphanumeric + underscore only",
            req.name
        );
    }

    let script_path = format!("{}/{}.py", PLUGIN_DIR, req.name);
    let metadata_path = format!("{}/{}.meta.json", PLUGIN_DIR, req.name);

    let mut deleted = Vec::new();

    if std::path::Path::new(&script_path).exists() {
        std::fs::remove_file(&script_path)
            .with_context(|| format!("Failed to delete plugin script: {script_path}"))?;
        deleted.push(script_path);
    }

    if std::path::Path::new(&metadata_path).exists() {
        std::fs::remove_file(&metadata_path)
            .with_context(|| format!("Failed to delete plugin metadata: {metadata_path}"))?;
        deleted.push(metadata_path);
    }

    if deleted.is_empty() {
        bail!("Plugin '{}' not found in {}", req.name, PLUGIN_DIR);
    }

    info!("Deleted plugin '{}': {:?}", req.name, deleted);

    let output = DeleteOutput {
        success: true,
        deleted_files: deleted,
    };

    serde_json::to_vec(&output).context("Failed to serialize plugin.delete output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_empty_dir() {
        // When PLUGIN_DIR doesn't exist, should return empty list
        let result = execute_list(b"{}").unwrap();
        let output: ListOutput = serde_json::from_slice(&result).unwrap();
        assert_eq!(output.count, 0);
        assert!(output.plugins.is_empty());
    }

    #[test]
    fn test_delete_invalid_name() {
        let input = serde_json::to_vec(&serde_json::json!({
            "name": "bad/name"
        }))
        .unwrap();
        let result = execute_delete(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_empty_name() {
        let input = serde_json::to_vec(&serde_json::json!({
            "name": ""
        }))
        .unwrap();
        let result = execute_delete(&input);
        assert!(result.is_err());
    }
}
