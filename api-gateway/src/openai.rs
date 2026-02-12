//! OpenAI API client

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::proto::common::InferenceResponse;

/// OpenAI API client
pub struct OpenAiClient {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
    model: String,
}

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    max_tokens: i32,
    temperature: f32,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiResponse {
    id: String,
    choices: Vec<OpenAiChoice>,
    model: String,
    usage: OpenAiUsage,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
    finish_reason: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiResponseMessage {
    role: String,
    content: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiUsage {
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
}

impl OpenAiClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            base_url: "https://api.openai.com".to_string(),
            model: "gpt-4o".to_string(),
        }
    }

    pub fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Send an inference request to OpenAI
    pub async fn infer(
        &self,
        prompt: &str,
        system_prompt: &str,
        max_tokens: i32,
        temperature: f32,
    ) -> Result<InferenceResponse> {
        if !self.is_available() {
            bail!("OpenAI API key not configured");
        }

        let max_tokens = if max_tokens <= 0 { 4096 } else { max_tokens };
        let temperature = if temperature <= 0.0 { 0.3 } else { temperature };

        let mut messages = Vec::new();
        if !system_prompt.is_empty() {
            messages.push(OpenAiMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            });
        }
        messages.push(OpenAiMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        let request_body = OpenAiRequest {
            model: self.model.clone(),
            messages,
            max_tokens,
            temperature,
        };

        let start = std::time::Instant::now();

        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let latency = start.elapsed().as_millis() as i64;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("OpenAI API error {status}: {body}");
        }

        let openai_response: OpenAiResponse = response.json().await?;

        let text = openai_response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        let tokens_used = openai_response.usage.total_tokens;

        info!(
            "OpenAI response: {} tokens, {}ms latency",
            tokens_used, latency
        );

        Ok(InferenceResponse {
            text,
            tokens_used,
            latency_ms: latency,
            model_used: openai_response.model,
            intelligence_level: "strategic".to_string(),
        })
    }

    /// Calculate cost for a request
    pub fn calculate_cost(input_tokens: i32, output_tokens: i32) -> f64 {
        // GPT-4o pricing (approximate)
        let input_cost = input_tokens as f64 * 2.5 / 1_000_000.0;
        let output_cost = output_tokens as f64 * 10.0 / 1_000_000.0;
        input_cost + output_cost
    }
}
