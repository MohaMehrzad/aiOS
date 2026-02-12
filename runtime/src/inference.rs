//! Inference engine — calls the llama-server OpenAI-compatible API.
//!
//! Each managed model exposes `/v1/chat/completions` on its allocated port.
//! This module provides both single-shot and streaming inference wrappers.

use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, warn};

use crate::proto::runtime::{InferChunk, InferRequest, InferResponse};

// ---------------------------------------------------------------------------
// HTTP request / response types (llama.cpp OpenAI-compat API)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    messages: Vec<ChatMessage>,
    max_tokens: i32,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: Option<ChatMessage>,
    delta: Option<ChatDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct UsageInfo {
    total_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    prompt_tokens: Option<i32>,
}

// ---------------------------------------------------------------------------
// InferenceEngine
// ---------------------------------------------------------------------------

/// Stateless inference engine backed by an HTTP client.
pub struct InferenceEngine {
    http_client: reqwest::Client,
}

impl InferenceEngine {
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build reqwest client");

        Self { http_client }
    }

    // ------------------------------------------------------------------
    // Single-shot inference
    // ------------------------------------------------------------------

    /// Run a single-shot (non-streaming) inference call against the
    /// llama-server instance on `port`.
    pub async fn infer(
        &self,
        port: u16,
        model_name: &str,
        request: &InferRequest,
    ) -> Result<InferResponse> {
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

        let messages = build_messages(&request.system_prompt, &request.prompt);
        let max_tokens = if request.max_tokens > 0 {
            request.max_tokens
        } else {
            512
        };
        let temperature = if request.temperature > 0.0 {
            request.temperature
        } else {
            0.7
        };

        let body = ChatCompletionRequest {
            messages,
            max_tokens,
            temperature,
            stream: false,
        };

        info!(
            model = %model_name,
            port,
            max_tokens,
            temperature,
            agent = %request.requesting_agent,
            task = %request.task_id,
            "Sending inference request"
        );

        let start = Instant::now();

        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("HTTP request to llama-server on port {port} failed"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            bail!(
                "llama-server returned HTTP {status} on port {port}: {body_text}"
            );
        }

        let completion: ChatCompletionResponse = resp
            .json()
            .await
            .context("Failed to parse ChatCompletionResponse JSON")?;

        let latency_ms = start.elapsed().as_millis() as i64;

        let text = completion
            .choices
            .first()
            .and_then(|c| c.message.as_ref())
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let tokens_used = completion
            .usage
            .as_ref()
            .and_then(|u| u.total_tokens)
            .unwrap_or(0);

        debug!(
            model = %model_name,
            tokens_used,
            latency_ms,
            "Inference complete"
        );

        Ok(InferResponse {
            text,
            tokens_used,
            latency_ms,
            model_used: model_name.to_string(),
        })
    }

    // ------------------------------------------------------------------
    // Streaming inference
    // ------------------------------------------------------------------

    /// Run a streaming inference call.  Returns a `ReceiverStream` that yields
    /// `InferChunk` items as the llama-server produces them via SSE.
    pub async fn stream_infer(
        &self,
        port: u16,
        model_name: &str,
        request: &InferRequest,
    ) -> Result<ReceiverStream<Result<InferChunk, tonic::Status>>> {
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

        let messages = build_messages(&request.system_prompt, &request.prompt);
        let max_tokens = if request.max_tokens > 0 {
            request.max_tokens
        } else {
            512
        };
        let temperature = if request.temperature > 0.0 {
            request.temperature
        } else {
            0.7
        };

        let body = ChatCompletionRequest {
            messages,
            max_tokens,
            temperature,
            stream: true,
        };

        info!(
            model = %model_name,
            port,
            max_tokens,
            temperature,
            agent = %request.requesting_agent,
            task = %request.task_id,
            "Starting streaming inference"
        );

        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| {
                format!("HTTP request to llama-server on port {port} failed (stream)")
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            bail!(
                "llama-server returned HTTP {status} on port {port} (stream): {body_text}"
            );
        }

        let (tx, rx) = mpsc::channel::<Result<InferChunk, tonic::Status>>(128);

        let model_owned = model_name.to_string();
        let byte_stream = resp;

        tokio::spawn(async move {
            // The llama.cpp streaming response uses SSE: each event is prefixed
            // with "data: " and separated by double newlines.  The terminal
            // event is "data: [DONE]".
            let full_body = match byte_stream.text().await {
                Ok(b) => b,
                Err(e) => {
                    error!(model = %model_owned, "Failed to read stream body: {e}");
                    let _ = tx
                        .send(Err(tonic::Status::internal(format!(
                            "Stream read error: {e}"
                        ))))
                        .await;
                    return;
                }
            };

            for line in full_body.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Strip "data: " prefix.
                let data = if let Some(stripped) = line.strip_prefix("data: ") {
                    stripped.trim()
                } else if let Some(stripped) = line.strip_prefix("data:") {
                    stripped.trim()
                } else {
                    // May be a raw JSON line in some llama.cpp versions.
                    line
                };

                if data == "[DONE]" {
                    let _ = tx
                        .send(Ok(InferChunk {
                            text: String::new(),
                            done: true,
                        }))
                        .await;
                    debug!(model = %model_owned, "Stream complete");
                    return;
                }

                // Parse JSON chunk.
                match serde_json::from_str::<ChatCompletionResponse>(data) {
                    Ok(chunk) => {
                        let text = chunk
                            .choices
                            .first()
                            .and_then(|c| c.delta.as_ref())
                            .and_then(|d| d.content.clone())
                            .unwrap_or_default();

                        let is_done = chunk
                            .choices
                            .first()
                            .and_then(|c| c.finish_reason.as_ref())
                            .is_some();

                        if tx
                            .send(Ok(InferChunk {
                                text,
                                done: is_done,
                            }))
                            .await
                            .is_err()
                        {
                            warn!(model = %model_owned, "Stream receiver dropped");
                            return;
                        }

                        if is_done {
                            debug!(model = %model_owned, "Stream finished (finish_reason)");
                            return;
                        }
                    }
                    Err(e) => {
                        // Non-fatal: some lines may be comments or empty.
                        debug!(
                            model = %model_owned,
                            line = data,
                            "Skipping unparseable SSE line: {e}"
                        );
                    }
                }
            }

            // If we exhaust the body without a [DONE] marker, send a final
            // done chunk to close the stream gracefully.
            let _ = tx
                .send(Ok(InferChunk {
                    text: String::new(),
                    done: true,
                }))
                .await;
        });

        Ok(ReceiverStream::new(rx))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_messages(system_prompt: &str, user_prompt: &str) -> Vec<ChatMessage> {
    let mut msgs = Vec::with_capacity(2);
    if !system_prompt.is_empty() {
        msgs.push(ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        });
    }
    msgs.push(ChatMessage {
        role: "user".to_string(),
        content: user_prompt.to_string(),
    });
    msgs
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_messages_with_system() {
        let msgs = build_messages("You are helpful.", "Hello!");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "You are helpful.");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[1].content, "Hello!");
    }

    #[test]
    fn test_build_messages_without_system() {
        let msgs = build_messages("", "Hello!");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn test_chat_completion_response_deserialize() {
        let json = r#"{
            "choices": [{
                "message": {"role": "assistant", "content": "Hi there!"},
                "finish_reason": "stop"
            }],
            "usage": {"total_tokens": 42, "completion_tokens": 10, "prompt_tokens": 32}
        }"#;
        let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.as_ref().unwrap().content,
            "Hi there!"
        );
        assert_eq!(resp.usage.as_ref().unwrap().total_tokens, Some(42));
    }

    #[test]
    fn test_chat_completion_stream_chunk_deserialize() {
        let json = r#"{
            "choices": [{
                "delta": {"content": "Hello"},
                "finish_reason": null
            }]
        }"#;
        let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.choices[0].delta.as_ref().unwrap().content,
            Some("Hello".to_string())
        );
        assert!(resp.choices[0].finish_reason.is_none());
    }

    #[test]
    fn test_chat_completion_done_chunk_deserialize() {
        let json = r#"{
            "choices": [{
                "delta": {},
                "finish_reason": "stop"
            }]
        }"#;
        let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    #[test]
    fn test_inference_engine_creation() {
        let engine = InferenceEngine::new();
        // Just ensure it doesn't panic.
        drop(engine);
    }

    #[test]
    fn test_chat_request_serialization() {
        let req = ChatCompletionRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "test".to_string(),
            }],
            max_tokens: 100,
            temperature: 0.5,
            stream: false,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["stream"], false);
        assert_eq!(json["max_tokens"], 100);
        assert_eq!(json["messages"][0]["role"], "user");
    }
}
