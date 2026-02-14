//! Plugin Event Dispatcher
//!
//! Background loop that monitors trigger conditions and fires plugins.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Trigger types that can activate a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TriggerType {
    #[serde(rename = "cron")]
    Cron { expression: String },
    #[serde(rename = "file_watch")]
    FileWatch { path: String },
    #[serde(rename = "log_pattern")]
    LogPattern { pattern: String, log_path: String },
    #[serde(rename = "metric_threshold")]
    MetricThreshold {
        metric: String,
        operator: String,
        threshold: f64,
    },
}

/// A registered plugin trigger
#[derive(Debug, Clone)]
pub struct PluginTrigger {
    pub id: String,
    pub plugin_name: String,
    pub trigger_type: TriggerType,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub last_fired: Option<i64>,
}

/// Event dispatcher that checks triggers and fires plugins
pub struct EventDispatcher {
    triggers: HashMap<String, PluginTrigger>,
    db_path: String,
}

impl EventDispatcher {
    pub fn new(db_path: &str) -> Self {
        Self {
            triggers: HashMap::new(),
            db_path: db_path.to_string(),
        }
    }

    /// Load triggers from database
    pub fn load_triggers(&mut self) -> Result<()> {
        let conn =
            rusqlite::Connection::open(&self.db_path).context("Failed to open triggers DB")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_triggers (
                id TEXT PRIMARY KEY,
                plugin_name TEXT NOT NULL,
                trigger_type TEXT NOT NULL,
                config_json TEXT DEFAULT '{}',
                enabled INTEGER DEFAULT 1,
                last_fired INTEGER
            )",
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, plugin_name, trigger_type, config_json, enabled, last_fired FROM plugin_triggers WHERE enabled = 1",
        )?;

        let triggers: Vec<PluginTrigger> = stmt
            .query_map([], |row| {
                let config_str: String = row.get(3)?;
                let config = serde_json::from_str(&config_str).unwrap_or_default();
                let trigger_type_str: String = row.get(2)?;
                let trigger_type: TriggerType =
                    serde_json::from_str(&trigger_type_str).unwrap_or(TriggerType::Cron {
                        expression: "0 * * * *".to_string(),
                    });

                Ok(PluginTrigger {
                    id: row.get(0)?,
                    plugin_name: row.get(1)?,
                    trigger_type,
                    config,
                    enabled: row.get::<_, i32>(4)? != 0,
                    last_fired: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        for trigger in triggers {
            self.triggers.insert(trigger.id.clone(), trigger);
        }

        info!("Loaded {} plugin triggers", self.triggers.len());
        Ok(())
    }

    /// Register a new trigger
    pub fn register_trigger(&mut self, trigger: PluginTrigger) -> Result<()> {
        let conn = rusqlite::Connection::open(&self.db_path)?;
        let trigger_type_json = serde_json::to_string(&trigger.trigger_type)?;
        let config_json = serde_json::to_string(&trigger.config)?;

        conn.execute(
            "INSERT OR REPLACE INTO plugin_triggers (id, plugin_name, trigger_type, config_json, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![trigger.id, trigger.plugin_name, trigger_type_json, config_json, trigger.enabled as i32],
        )?;

        self.triggers.insert(trigger.id.clone(), trigger);
        Ok(())
    }

    /// Remove a trigger
    pub fn remove_trigger(&mut self, trigger_id: &str) -> Result<()> {
        let conn = rusqlite::Connection::open(&self.db_path)?;
        conn.execute("DELETE FROM plugin_triggers WHERE id = ?1", [trigger_id])?;
        self.triggers.remove(trigger_id);
        Ok(())
    }

    /// Get all active triggers
    pub fn list_triggers(&self) -> Vec<&PluginTrigger> {
        self.triggers.values().collect()
    }

    /// Run the event dispatch loop
    pub async fn run(dispatcher: Arc<RwLock<Self>>, cancel: tokio_util::sync::CancellationToken) {
        info!("Plugin event dispatcher started");
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Plugin event dispatcher shutting down");
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                    let disp = dispatcher.read().await;
                    let trigger_count = disp.triggers.len();
                    if trigger_count > 0 {
                        debug!("Checking {} plugin triggers", trigger_count);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_dispatcher_new() {
        let dispatcher = EventDispatcher::new("/tmp/test_triggers.db");
        assert!(dispatcher.triggers.is_empty());
    }
}
