//! Budget Manager — tracks API spending and enforces limits

use chrono::Datelike;
use tracing::{info, warn};

use crate::proto::api_gateway::{BudgetStatus, UsageRecord, UsageResponse};

/// Tracks API usage and enforces budget limits
pub struct BudgetManager {
    claude_monthly_budget: f64,
    openai_monthly_budget: f64,
    claude_used: f64,
    openai_used: f64,
    usage_records: Vec<UsageRecord>,
    month_start: i64,
}

impl BudgetManager {
    pub fn new(claude_budget: f64, openai_budget: f64) -> Self {
        Self {
            claude_monthly_budget: claude_budget,
            openai_monthly_budget: openai_budget,
            claude_used: 0.0,
            openai_used: 0.0,
            usage_records: Vec::new(),
            month_start: current_month_start(),
        }
    }

    /// Record API usage
    pub fn record_usage(&mut self, provider: &str, tokens: i32, model: &str) {
        self.maybe_reset_monthly();

        let cost = match provider {
            "claude" => {
                // Rough estimate: split tokens 50/50 input/output
                let cost = crate::claude::ClaudeClient::calculate_cost(tokens / 2, tokens / 2);
                self.claude_used += cost;
                cost
            }
            "openai" => {
                let cost = crate::openai::OpenAiClient::calculate_cost(tokens / 2, tokens / 2);
                self.openai_used += cost;
                cost
            }
            _ => 0.0,
        };

        self.usage_records.push(UsageRecord {
            provider: provider.to_string(),
            model: model.to_string(),
            input_tokens: tokens / 2,
            output_tokens: tokens / 2,
            cost_usd: cost,
            timestamp: chrono::Utc::now().timestamp(),
            requesting_agent: String::new(),
            task_id: String::new(),
        });

        info!(
            "API usage: provider={provider} tokens={tokens} cost=${cost:.4} total_claude=${:.2} total_openai=${:.2}",
            self.claude_used, self.openai_used
        );

        // Warn if approaching budget
        if self.claude_used > self.claude_monthly_budget * 0.8 {
            warn!(
                "Claude budget warning: ${:.2} / ${:.2} ({}%)",
                self.claude_used,
                self.claude_monthly_budget,
                (self.claude_used / self.claude_monthly_budget * 100.0) as u32
            );
        }
    }

    /// Check if overall budget is exceeded
    pub fn is_budget_exceeded(&self) -> bool {
        self.claude_used >= self.claude_monthly_budget
            && self.openai_used >= self.openai_monthly_budget
    }

    /// Check if a specific provider's budget is exceeded
    pub fn is_provider_budget_exceeded(&self, provider: &str) -> bool {
        match provider {
            "claude" => self.claude_used >= self.claude_monthly_budget,
            "openai" => self.openai_used >= self.openai_monthly_budget,
            _ => true,
        }
    }

    /// Get budget status
    pub fn get_status(&self) -> BudgetStatus {
        let now = chrono::Utc::now();
        let days_in_month = 30; // Approximate
        let day_of_month = now.format("%d").to_string().parse::<i32>().unwrap_or(1);
        let days_remaining = (days_in_month - day_of_month).max(0);

        let total_used = self.claude_used + self.openai_used;
        let daily_rate = if day_of_month > 0 {
            total_used / day_of_month as f64
        } else {
            0.0
        };

        BudgetStatus {
            claude_monthly_budget_usd: self.claude_monthly_budget,
            claude_used_usd: self.claude_used,
            openai_monthly_budget_usd: self.openai_monthly_budget,
            openai_used_usd: self.openai_used,
            days_remaining,
            daily_rate_usd: daily_rate,
            budget_exceeded: self.is_budget_exceeded(),
        }
    }

    /// Get usage records
    pub fn get_usage(&self, provider: &str, days: i32) -> UsageResponse {
        let cutoff = chrono::Utc::now().timestamp() - (days as i64 * 86400);

        let records: Vec<UsageRecord> = self
            .usage_records
            .iter()
            .filter(|r| (provider.is_empty() || r.provider == provider) && r.timestamp >= cutoff)
            .cloned()
            .collect();

        let total_cost: f64 = records.iter().map(|r| r.cost_usd).sum();
        let total_requests = records.len() as i32;
        let total_tokens: i32 = records
            .iter()
            .map(|r| r.input_tokens + r.output_tokens)
            .sum();

        UsageResponse {
            records,
            total_cost_usd: total_cost,
            total_requests,
            total_tokens,
        }
    }

    /// Pre-check: reject requests that would exceed the budget
    /// Returns Ok(()) if the request can proceed, Err with reason if rejected
    pub fn pre_check(&self, provider: &str) -> Result<(), String> {
        if self.is_budget_exceeded() {
            return Err("All API budgets exceeded for this billing period".to_string());
        }

        if self.is_provider_budget_exceeded(provider) {
            let (used, budget) = match provider {
                "claude" => (self.claude_used, self.claude_monthly_budget),
                "openai" => (self.openai_used, self.openai_monthly_budget),
                _ => return Err(format!("Unknown provider: {provider}")),
            };
            return Err(format!(
                "{provider} budget exceeded: ${used:.2} / ${budget:.2}"
            ));
        }

        Ok(())
    }

    /// Get remaining budget for a provider
    pub fn remaining_budget(&self, provider: &str) -> f64 {
        match provider {
            "claude" => (self.claude_monthly_budget - self.claude_used).max(0.0),
            "openai" => (self.openai_monthly_budget - self.openai_used).max(0.0),
            _ => 0.0,
        }
    }

    /// Reset monthly counters if we're in a new month
    fn maybe_reset_monthly(&mut self) {
        let current_start = current_month_start();
        if current_start > self.month_start {
            info!(
                "New billing month — resetting counters (previous: claude=${:.2} openai=${:.2})",
                self.claude_used, self.openai_used
            );
            self.claude_used = 0.0;
            self.openai_used = 0.0;
            self.month_start = current_start;
            self.usage_records.clear();
        }
    }
}

/// Get the Unix timestamp for the start of the current month
fn current_month_start() -> i64 {
    let now = chrono::Utc::now();
    let start = now.date_naive().with_day(1).unwrap_or(now.date_naive());
    start
        .and_hms_opt(0, 0, 0)
        .map(|dt: chrono::NaiveDateTime| dt.and_utc().timestamp())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_tracking() {
        let mut bm = BudgetManager::new(10.0, 5.0);
        assert!(!bm.is_budget_exceeded());

        bm.record_usage("claude", 1000, "claude-sonnet");
        assert!(!bm.is_budget_exceeded());

        let status = bm.get_status();
        assert!(status.claude_used_usd > 0.0);
        assert!(!status.budget_exceeded);
    }

    #[test]
    fn test_usage_records() {
        let mut bm = BudgetManager::new(100.0, 50.0);
        bm.record_usage("claude", 1000, "claude-sonnet");
        bm.record_usage("openai", 500, "gpt-4o");

        let usage = bm.get_usage("", 30);
        assert_eq!(usage.total_requests, 2);
        assert!(usage.total_tokens > 0);
    }

    #[test]
    fn test_is_budget_exceeded_both() {
        // Budget is only exceeded when BOTH providers are exceeded
        let mut bm = BudgetManager::new(0.0001, 0.0001); // tiny budgets
        bm.record_usage("claude", 100000, "claude-sonnet");
        // Only claude exceeded
        assert!(!bm.is_budget_exceeded()); // Not both exceeded yet

        bm.record_usage("openai", 100000, "gpt-4o");
        // Now both should be exceeded
        assert!(bm.is_budget_exceeded());
    }

    #[test]
    fn test_is_provider_budget_exceeded_claude() {
        let mut bm = BudgetManager::new(0.0001, 100.0); // tiny claude budget
        bm.record_usage("claude", 100000, "claude-sonnet");
        assert!(bm.is_provider_budget_exceeded("claude"));
        assert!(!bm.is_provider_budget_exceeded("openai"));
    }

    #[test]
    fn test_is_provider_budget_exceeded_openai() {
        let mut bm = BudgetManager::new(100.0, 0.0001); // tiny openai budget
        bm.record_usage("openai", 100000, "gpt-4o");
        assert!(!bm.is_provider_budget_exceeded("claude"));
        assert!(bm.is_provider_budget_exceeded("openai"));
    }

    #[test]
    fn test_is_provider_budget_exceeded_unknown() {
        let bm = BudgetManager::new(100.0, 50.0);
        // Unknown provider is always considered exceeded
        assert!(bm.is_provider_budget_exceeded("unknown"));
    }

    #[test]
    fn test_get_status_fields() {
        let bm = BudgetManager::new(100.0, 50.0);
        let status = bm.get_status();
        assert_eq!(status.claude_monthly_budget_usd, 100.0);
        assert_eq!(status.openai_monthly_budget_usd, 50.0);
        assert_eq!(status.claude_used_usd, 0.0);
        assert_eq!(status.openai_used_usd, 0.0);
        assert!(!status.budget_exceeded);
    }

    #[test]
    fn test_get_status_after_usage() {
        let mut bm = BudgetManager::new(100.0, 50.0);
        bm.record_usage("claude", 1000, "claude-sonnet");

        let status = bm.get_status();
        assert!(status.claude_used_usd > 0.0);
        assert_eq!(status.openai_used_usd, 0.0);
        assert!(status.daily_rate_usd > 0.0);
    }

    #[test]
    fn test_get_usage_filter_by_provider() {
        let mut bm = BudgetManager::new(100.0, 50.0);
        bm.record_usage("claude", 1000, "claude-sonnet");
        bm.record_usage("openai", 500, "gpt-4o");
        bm.record_usage("claude", 2000, "claude-sonnet");

        let claude_usage = bm.get_usage("claude", 30);
        assert_eq!(claude_usage.total_requests, 2);

        let openai_usage = bm.get_usage("openai", 30);
        assert_eq!(openai_usage.total_requests, 1);

        let all_usage = bm.get_usage("", 30);
        assert_eq!(all_usage.total_requests, 3);
    }

    #[test]
    fn test_usage_cost_calculation() {
        let mut bm = BudgetManager::new(100.0, 50.0);
        bm.record_usage("claude", 2000, "claude-sonnet");

        let usage = bm.get_usage("claude", 30);
        assert_eq!(usage.total_requests, 1);
        // 2000 tokens split 50/50 = 1000 input, 1000 output
        // Claude cost: 1000 * 3.0 / 1M + 1000 * 15.0 / 1M = 0.003 + 0.015 = 0.018
        let expected_cost = 1000.0 * 3.0 / 1_000_000.0 + 1000.0 * 15.0 / 1_000_000.0;
        assert!((usage.total_cost_usd - expected_cost).abs() < 0.0001);
    }

    #[test]
    fn test_record_unknown_provider() {
        let mut bm = BudgetManager::new(100.0, 50.0);
        bm.record_usage("unknown", 1000, "model-x");

        // Should still record it
        let usage = bm.get_usage("unknown", 30);
        assert_eq!(usage.total_requests, 1);
        assert_eq!(usage.total_cost_usd, 0.0); // Unknown provider = 0 cost
    }

    #[test]
    fn test_usage_records_contain_correct_fields() {
        let mut bm = BudgetManager::new(100.0, 50.0);
        bm.record_usage("claude", 1000, "claude-sonnet");

        let usage = bm.get_usage("", 30);
        assert_eq!(usage.records.len(), 1);
        let record = &usage.records[0];
        assert_eq!(record.provider, "claude");
        assert_eq!(record.model, "claude-sonnet");
        assert_eq!(record.input_tokens, 500);
        assert_eq!(record.output_tokens, 500);
        assert!(record.cost_usd > 0.0);
        assert!(record.timestamp > 0);
    }

    #[test]
    fn test_pre_check_within_budget() {
        let bm = BudgetManager::new(100.0, 50.0);
        assert!(bm.pre_check("claude").is_ok());
        assert!(bm.pre_check("openai").is_ok());
    }

    #[test]
    fn test_pre_check_exceeded() {
        let mut bm = BudgetManager::new(0.0001, 100.0);
        bm.record_usage("claude", 100000, "claude-sonnet");
        assert!(bm.pre_check("claude").is_err());
        assert!(bm.pre_check("openai").is_ok());
    }

    #[test]
    fn test_pre_check_unknown_provider() {
        let bm = BudgetManager::new(100.0, 50.0);
        // Unknown provider is not exceeded since is_budget_exceeded checks both
        assert!(bm.pre_check("unknown").is_err());
    }

    #[test]
    fn test_remaining_budget() {
        let mut bm = BudgetManager::new(100.0, 50.0);
        assert_eq!(bm.remaining_budget("claude"), 100.0);
        bm.record_usage("claude", 1000, "claude-sonnet");
        assert!(bm.remaining_budget("claude") < 100.0);
        assert_eq!(bm.remaining_budget("openai"), 50.0);
        assert_eq!(bm.remaining_budget("unknown"), 0.0);
    }

    #[test]
    fn test_initial_state() {
        let bm = BudgetManager::new(100.0, 50.0);
        assert!(!bm.is_budget_exceeded());
        assert!(!bm.is_provider_budget_exceeded("claude"));
        assert!(!bm.is_provider_budget_exceeded("openai"));

        let usage = bm.get_usage("", 30);
        assert_eq!(usage.total_requests, 0);
        assert_eq!(usage.total_cost_usd, 0.0);
        assert_eq!(usage.total_tokens, 0);
    }
}
