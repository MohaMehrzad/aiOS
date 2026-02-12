//! Claude API client

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::proto::common::InferenceResponse;

/// Claude API client
pub struct ClaudeClient {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
    model: String,
}

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: i32,
    temperature: f32,
    system: String,
    messages: Vec<ClaudeMessage>,
}

#[derive(Serialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ClaudeResponse {
    id: String,
    content: Vec<ClaudeContent>,
    model: String,
    usage: ClaudeUsage,
}

#[derive(Deserialize)]
struct ClaudeContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Deserialize)]
struct ClaudeUsage {
    input_tokens: i32,
    output_tokens: i32,
}

impl ClaudeClient {
    pub fn new(api_key: String) -> Self {
        let model = std::env::var("CLAUDE_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
        Self {
            api_key,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            base_url: "https://api.anthropic.com".to_string(),
            model,
        }
    }

    /// Get the model name this client is configured for
    pub fn model_name(&self) -> &str {
        &self.model
    }

    pub fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Send an inference request to Claude
    pub async fn infer(
        &self,
        prompt: &str,
        system_prompt: &str,
        max_tokens: i32,
        temperature: f32,
    ) -> Result<InferenceResponse> {
        if !self.is_available() {
            bail!("Claude API key not configured");
        }

        let max_tokens = if max_tokens <= 0 { 4096 } else { max_tokens };
        let temperature = if temperature <= 0.0 { 0.3 } else { temperature };

        let request_body = ClaudeRequest {
            model: self.model.clone(),
            max_tokens,
            temperature,
            system: system_prompt.to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let start = std::time::Instant::now();

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let latency = start.elapsed().as_millis() as i64;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Claude API error {status}: {body}");
        }

        let claude_response: ClaudeResponse = response.json().await?;

        let text = claude_response
            .content
            .into_iter()
            .filter(|c| c.content_type == "text")
            .map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        let tokens_used =
            claude_response.usage.input_tokens + claude_response.usage.output_tokens;

        info!(
            "Claude response: {} tokens, {}ms latency",
            tokens_used, latency
        );

        Ok(InferenceResponse {
            text,
            tokens_used,
            latency_ms: latency,
            model_used: claude_response.model,
            intelligence_level: "strategic".to_string(),
        })
    }

    /// Calculate cost for a request
    pub fn calculate_cost(input_tokens: i32, output_tokens: i32) -> f64 {
        // Claude Sonnet pricing (approximate)
        let input_cost = input_tokens as f64 * 3.0 / 1_000_000.0;
        let output_cost = output_tokens as f64 * 15.0 / 1_000_000.0;
        input_cost + output_cost
    }
}
