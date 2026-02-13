//! Proactive Goal Generator
//!
//! Background task that monitors system state and autonomously creates
//! goals when conditions warrant action. Runs alongside the autonomy loop.
//!
//! Monitors:
//! - System health metrics (CPU, memory, disk)
//! - Service availability
//! - Security issues
//! - Available updates
//!
//! Deduplicates goals: won't create one if a similar goal is already active.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::OrchestratorState;

/// Configuration for the proactive goal generator
pub struct ProactiveConfig {
    /// How often to check for proactive goals
    pub check_interval: Duration,
    /// CPU threshold (%) to trigger a goal
    pub cpu_threshold: f64,
    /// Memory usage threshold (%) to trigger a goal
    pub memory_threshold: f64,
    /// Disk usage threshold (%) to trigger a goal
    pub disk_threshold: f64,
}

impl Default for ProactiveConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(60),
            cpu_threshold: 90.0,
            memory_threshold: 85.0,
            disk_threshold: 90.0,
        }
    }
}

/// Run the proactive goal generator loop
pub async fn run_proactive_loop(
    state: Arc<RwLock<OrchestratorState>>,
    cancel: CancellationToken,
    config: ProactiveConfig,
) {
    info!(
        "Proactive goal generator started (interval={}s)",
        config.check_interval.as_secs()
    );

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("Proactive goal generator shutting down");
                break;
            }
            _ = tokio::time::sleep(config.check_interval) => {
                if let Err(e) = proactive_check(&state, &config).await {
                    error!("Proactive check error: {e}");
                }
            }
        }
    }

    info!("Proactive goal generator stopped");
}

/// Single proactive check iteration
async fn proactive_check(
    state: &Arc<RwLock<OrchestratorState>>,
    config: &ProactiveConfig,
) -> anyhow::Result<()> {
    debug!("Running proactive system check");

    let mut goals_to_create: Vec<(String, i32)> = Vec::new();

    // Check CPU usage
    let cpu = crate::read_cpu_percent();
    if cpu > config.cpu_threshold {
        goals_to_create.push((
            format!(
                "Investigate high CPU usage ({cpu:.1}% > {:.0}% threshold). \
                 Identify top processes and take corrective action.",
                config.cpu_threshold
            ),
            7,
        ));
    }

    // Check memory usage
    let (mem_used, mem_total) = crate::read_memory_mb();
    if mem_total > 0.0 {
        let mem_percent = (mem_used / mem_total) * 100.0;
        if mem_percent > config.memory_threshold {
            goals_to_create.push((
                format!(
                    "Investigate high memory usage ({mem_percent:.1}% > {:.0}% threshold). \
                     Identify memory-heavy processes and free memory.",
                    config.memory_threshold
                ),
                7,
            ));
        }
    }

    // Check disk space (via /proc/mounts on Linux, or df)
    let disk_percent = read_disk_usage_percent();
    if disk_percent > config.disk_threshold {
        goals_to_create.push((
            format!(
                "Disk usage critically high ({disk_percent:.1}% > {:.0}% threshold). \
                 Clean up temporary files, old logs, and unnecessary data.",
                config.disk_threshold
            ),
            8,
        ));
    }

    // Check for failed services by looking at agent health
    let state_r = state.read().await;
    let agents = state_r.agent_router.list_agents().await;
    let total_agents = agents.len();
    let failed_agents: Vec<_> = agents
        .iter()
        .filter(|a| a.status == "failed" || a.status == "unresponsive")
        .collect();

    if !failed_agents.is_empty() {
        let names: Vec<String> = failed_agents.iter().map(|a| a.agent_id.clone()).collect();
        goals_to_create.push((
            format!(
                "Restart failed agents: {}. Investigate root cause.",
                names.join(", ")
            ),
            8,
        ));
    }

    // Check inter-service health (uses health checker results)
    let health_statuses = state_r.health_checker.read().await.get_all_status();
    let unhealthy: Vec<_> = health_statuses
        .iter()
        .filter(|s| !s.healthy && s.consecutive_failures >= 3)
        .collect();
    if !unhealthy.is_empty() {
        let names: Vec<String> = unhealthy.iter().map(|s| s.name.clone()).collect();
        goals_to_create.push((
            format!(
                "Services unhealthy: {}. Restart and investigate root cause.",
                names.join(", ")
            ),
            9,
        ));
    }

    // Check certificate expiry (files older than 30 days)
    let cert_path = "/var/lib/aios/certs/server.crt";
    if let Ok(meta) = std::fs::metadata(cert_path) {
        if let Ok(modified) = meta.modified() {
            if let Ok(age) = modified.elapsed() {
                let days_old = age.as_secs() / 86400;
                if days_old > 30 {
                    goals_to_create.push((
                        format!(
                            "TLS certificate is {days_old} days old. Rotate certificates \
                             using sec.cert_rotate before expiry."
                        ),
                        7,
                    ));
                }
            }
        }
    }

    // Check backup schedule (last backup timestamp)
    let backup_status_path = "/var/lib/aios/backup_status.json";
    if let Ok(contents) = std::fs::read_to_string(backup_status_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&contents) {
            if let Some(last_ts) = val.get("last_backup_timestamp").and_then(|v| v.as_i64()) {
                let now_ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let hours_since = (now_ts - last_ts) / 3600;
                if hours_since > 24 {
                    goals_to_create.push((
                        format!(
                            "No backup in {hours_since} hours. Run system backup \
                             to protect data and configurations."
                        ),
                        6,
                    ));
                }
            }
        }
    }

    // Check network connectivity (DNS resolution)
    let dns_ok = std::process::Command::new("nslookup")
        .args(["1.1.1.1"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !dns_ok {
        // Fallback: try ping
        let ping_ok = std::process::Command::new("ping")
            .args(["-c", "1", "-W", "2", "1.1.1.1"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ping_ok {
            goals_to_create.push((
                "Network connectivity issue: DNS and ping to 1.1.1.1 failed. \
                 Diagnose network configuration and restore connectivity."
                    .to_string(),
                9,
            ));
        }
    }

    // Check for log anomalies (count ERROR/CRITICAL in recent logs)
    let log_path = "/var/log/aios/orchestrator.log";
    if let Ok(log_contents) = std::fs::read_to_string(log_path) {
        let recent_lines: Vec<&str> = log_contents.lines().rev().take(500).collect();
        let error_count = recent_lines
            .iter()
            .filter(|l| l.contains("ERROR") || l.contains("CRITICAL"))
            .count();
        if error_count > 50 {
            goals_to_create.push((
                format!(
                    "Log anomaly: {error_count} ERROR/CRITICAL entries in recent logs. \
                     Investigate root cause and resolve recurring errors."
                ),
                7,
            ));
        }
    }

    // Check for available security vulnerabilities (world-writable files in /etc)
    let ww_count = std::process::Command::new("find")
        .args(["/etc", "-maxdepth", "2", "-perm", "-o+w", "-type", "f"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .count()
        })
        .unwrap_or(0);
    if ww_count > 0 {
        goals_to_create.push((
            format!(
                "Security: {ww_count} world-writable files found in /etc. \
                 Fix file permissions to prevent unauthorized modification."
            ),
            8,
        ));
    }

    drop(state_r);

    // Submit goals, deduplicating against active goals
    if goals_to_create.is_empty() {
        debug!("Proactive check: all clear ({total_agents} agents healthy)");
        return Ok(());
    }

    let mut state_w = state.write().await;

    for (description, priority) in goals_to_create {
        // Check for duplicate: skip if a similar goal is already active
        if has_similar_active_goal(&state_w, &description).await {
            debug!("Skipping duplicate proactive goal: {}", &description[..60.min(description.len())]);
            continue;
        }

        match state_w
            .goal_engine
            .submit_goal(description.clone(), priority, "proactive-monitor".to_string())
            .await
        {
            Ok(goal_id) => {
                info!("Proactive goal created: {goal_id} — {}", &description[..80.min(description.len())]);

                // Decompose into tasks
                if let Ok(tasks) = state_w
                    .task_planner
                    .decompose_goal(&goal_id, &description)
                    .await
                {
                    state_w.goal_engine.add_tasks(&goal_id, tasks);
                }

                // Log the decision
                state_w.decision_logger.log_decision(
                    "proactive_goal",
                    &[goal_id],
                    "created",
                    &description,
                    "reactive",
                    "proactive-monitor",
                );
            }
            Err(e) => {
                warn!("Failed to create proactive goal: {e}");
            }
        }
    }

    Ok(())
}

/// Check if a similar goal is already active (simple keyword overlap check)
async fn has_similar_active_goal(state: &OrchestratorState, description: &str) -> bool {
    let (goals, _) = state.goal_engine.list_goals("", 100, 0).await;

    // Extract key terms from the new goal description
    let keywords: Vec<&str> = description
        .split_whitespace()
        .filter(|w| w.len() > 4)
        .take(5)
        .collect();

    for goal in goals {
        if goal.status == "completed" || goal.status == "cancelled" {
            continue;
        }

        // Check if most keywords match
        let matching = keywords
            .iter()
            .filter(|kw| goal.description.to_lowercase().contains(&kw.to_lowercase()))
            .count();

        if matching >= keywords.len().max(1) / 2 + 1 {
            return true;
        }
    }

    false
}

/// Read disk usage percentage for the root filesystem
fn read_disk_usage_percent() -> f64 {
    // Use df command — works on both Linux and macOS
    std::process::Command::new("df")
        .args(["-P", "/"])
        .output()
        .ok()
        .and_then(|output| {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            text.lines()
                .nth(1)
                .and_then(|l| {
                    l.split_whitespace()
                        .find(|w| w.ends_with('%'))
                        .and_then(|w| w.trim_end_matches('%').parse::<f64>().ok())
                })
        })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proactive_config_default() {
        let config = ProactiveConfig::default();
        assert_eq!(config.check_interval, Duration::from_secs(60));
        assert_eq!(config.cpu_threshold, 90.0);
        assert_eq!(config.memory_threshold, 85.0);
        assert_eq!(config.disk_threshold, 90.0);
    }

    #[test]
    fn test_read_disk_usage() {
        let percent = read_disk_usage_percent();
        // Should return a value between 0 and 100
        assert!(percent >= 0.0);
        assert!(percent <= 100.0);
    }
}
