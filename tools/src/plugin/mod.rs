//! Plugin System — runtime-extensible tools via Python scripts
//!
//! Allows the AI to create, manage, and execute plugin tools at runtime.
//! Plugins are Python scripts stored in PLUGIN_DIR with JSON metadata.
//! Each plugin defines a `def main(input_data: dict) -> dict` function
//! that receives/returns JSON via stdin/stdout.

pub mod create;
pub mod events;
pub mod manage;
pub mod templates;
pub mod triggers;
pub mod validate;

use crate::registry::{make_tool, Registry};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Directory where plugin scripts and metadata are stored
pub const PLUGIN_DIR: &str = "/var/lib/aios/plugins";

/// Metadata for a plugin tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub tool_name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub dependencies: Vec<String>,
    pub author: String,
    pub created_at: String,
    pub timeout_ms: i32,
    /// Plugin chaining: list of plugin names to execute after this one
    #[serde(default)]
    pub next_plugins: Vec<String>,
    /// How to pass output to chained plugins: "pipe" (default) or "merge"
    #[serde(default = "default_output_mode")]
    pub output_mode: String,
}

fn default_output_mode() -> String {
    "pipe".to_string()
}

/// Register the 4 meta-tools for plugin management
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "plugin.create",
        "plugin",
        "Create a new plugin tool from Python code. The AI writes a main(input_data) -> dict function.",
        vec!["plugin_manage", "fs_write"],
        "high",
        false,
        true,
        30000,
    ));

    reg.register_tool(make_tool(
        "plugin.list",
        "plugin",
        "List all installed plugin tools with their descriptions and metadata",
        vec!["plugin_read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "plugin.delete",
        "plugin",
        "Delete a plugin tool by name, removing its script and metadata files",
        vec!["plugin_manage"],
        "high",
        false,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "plugin.install_deps",
        "plugin",
        "Install Python pip dependencies for a plugin",
        vec!["plugin_manage", "pkg_manage"],
        "high",
        false,
        false,
        60000,
    ));

    reg.register_tool(make_tool(
        "plugin.from_template",
        "plugin",
        "Create a plugin from a pre-built template (web_scraper, log_analyzer, file_processor, api_client)",
        vec!["plugin_manage", "fs_write"],
        "medium",
        false,
        true,
        30000,
    ));
}

/// Scan PLUGIN_DIR for *.meta.json files and register each as a tool in the registry.
/// Called at startup and after plugin.create succeeds.
pub fn scan_and_register_plugins(reg: &mut Registry) {
    let plugin_dir = std::path::Path::new(PLUGIN_DIR);
    if !plugin_dir.exists() {
        info!(
            "Plugin directory {} does not exist, skipping scan",
            PLUGIN_DIR
        );
        return;
    }

    let entries = match std::fs::read_dir(plugin_dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Failed to read plugin directory: {e}");
            return;
        }
    };

    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json")
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or(false, |n| n.ends_with(".meta.json"))
        {
            match std::fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str::<PluginMetadata>(&contents) {
                    Ok(meta) => {
                        reg.register_tool(make_tool(
                            &meta.tool_name,
                            "plugin",
                            &meta.description,
                            meta.capabilities.iter().map(|s| s.as_str()).collect(),
                            "medium",
                            false,
                            false,
                            meta.timeout_ms,
                        ));
                        count += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse plugin metadata {}: {e}", path.display());
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read plugin metadata {}: {e}", path.display());
                }
            }
        }
    }

    if count > 0 {
        info!("Loaded {count} plugin tools from {}", PLUGIN_DIR);
    }
}

/// Start a filesystem watcher on PLUGIN_DIR for hot-reload of plugins.
/// When a .meta.json file is created or modified, re-scan and register plugins.
pub fn start_hot_reload_watcher(registry: Arc<Mutex<Registry>>) -> Option<RecommendedWatcher> {
    let plugin_dir = std::path::Path::new(PLUGIN_DIR);
    if !plugin_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(plugin_dir) {
            warn!("Cannot create plugin dir for hot-reload: {e}");
            return None;
        }
    }

    let reg = registry.clone();
    let mut watcher =
        match notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event) => {
                let dominated_by_meta = event
                    .paths
                    .iter()
                    .any(|p| p.to_str().map_or(false, |s| s.ends_with(".meta.json")));
                let dominated_by_py = event
                    .paths
                    .iter()
                    .any(|p| p.to_str().map_or(false, |s| s.ends_with(".py")));

                if dominated_by_meta || dominated_by_py {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            info!("Plugin hot-reload: detected change, rescanning");
                            // Block on acquiring the lock (this runs in notify's thread)
                            if let Ok(mut reg_lock) = reg.try_lock() {
                                scan_and_register_plugins(&mut reg_lock);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                warn!("Plugin hot-reload watcher error: {e}");
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to create plugin hot-reload watcher: {e}");
                return None;
            }
        };

    if let Err(e) = watcher.watch(plugin_dir, RecursiveMode::NonRecursive) {
        warn!("Failed to watch plugin directory: {e}");
        return None;
    }

    info!("Plugin hot-reload watcher started on {}", PLUGIN_DIR);
    Some(watcher)
}
