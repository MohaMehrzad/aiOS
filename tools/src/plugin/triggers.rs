//! Plugin Trigger Types and Evaluation
//!
//! Defines the trigger conditions that can activate plugins
//! and the logic to evaluate them.

use std::time::SystemTime;
use tracing::debug;

/// Evaluate a file_watch trigger — check if the file has been modified
pub fn check_file_watch(path: &str, last_checked: Option<i64>) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    match last_checked {
        Some(last) => modified > last,
        None => true,
    }
}

/// Simple cron expression matcher (minute hour day month weekday)
/// Supports: *, specific numbers, */N for intervals
pub fn check_cron(expression: &str, now: &chrono::DateTime<chrono::Utc>) -> bool {
    let parts: Vec<&str> = expression.split_whitespace().collect();
    if parts.len() != 5 {
        return false;
    }

    let checks = [
        (
            parts[0],
            now.format("%M").to_string().parse::<u32>().unwrap_or(0),
        ),
        (
            parts[1],
            now.format("%H").to_string().parse::<u32>().unwrap_or(0),
        ),
        (
            parts[2],
            now.format("%d").to_string().parse::<u32>().unwrap_or(0),
        ),
        (
            parts[3],
            now.format("%m").to_string().parse::<u32>().unwrap_or(0),
        ),
        (
            parts[4],
            now.format("%u").to_string().parse::<u32>().unwrap_or(0),
        ),
    ];

    for (pattern, value) in &checks {
        if !matches_cron_field(pattern, *value) {
            return false;
        }
    }

    true
}

/// Match a single cron field
fn matches_cron_field(pattern: &str, value: u32) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(interval_str) = pattern.strip_prefix("*/") {
        if let Ok(interval) = interval_str.parse::<u32>() {
            return interval > 0 && value % interval == 0;
        }
    }
    for part in pattern.split(',') {
        if let Ok(n) = part.trim().parse::<u32>() {
            if n == value {
                return true;
            }
        }
    }
    false
}

/// Check a metric threshold trigger
pub fn check_metric_threshold(current_value: f64, operator: &str, threshold: f64) -> bool {
    match operator {
        ">" | "gt" => current_value > threshold,
        ">=" | "gte" => current_value >= threshold,
        "<" | "lt" => current_value < threshold,
        "<=" | "lte" => current_value <= threshold,
        "==" | "eq" => (current_value - threshold).abs() < f64::EPSILON,
        "!=" | "ne" => (current_value - threshold).abs() >= f64::EPSILON,
        _ => {
            debug!("Unknown metric operator: {}", operator);
            false
        }
    }
}

/// Check a log pattern trigger
pub fn check_log_pattern(log_line: &str, pattern: &str) -> bool {
    log_line.contains(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_cron_wildcard() {
        assert!(matches_cron_field("*", 5));
        assert!(matches_cron_field("*", 0));
    }

    #[test]
    fn test_matches_cron_interval() {
        assert!(matches_cron_field("*/5", 0));
        assert!(matches_cron_field("*/5", 5));
        assert!(matches_cron_field("*/5", 10));
        assert!(!matches_cron_field("*/5", 3));
    }

    #[test]
    fn test_matches_cron_specific() {
        assert!(matches_cron_field("5", 5));
        assert!(!matches_cron_field("5", 3));
    }

    #[test]
    fn test_check_metric_threshold() {
        assert!(check_metric_threshold(95.0, ">", 90.0));
        assert!(!check_metric_threshold(85.0, ">", 90.0));
        assert!(check_metric_threshold(90.0, ">=", 90.0));
        assert!(check_metric_threshold(85.0, "<", 90.0));
    }

    #[test]
    fn test_check_log_pattern() {
        assert!(check_log_pattern("ERROR: disk full", "ERROR"));
        assert!(!check_log_pattern("INFO: all good", "ERROR"));
    }
}
