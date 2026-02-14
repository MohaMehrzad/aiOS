//! Request Router — selects provider based on preference, availability, budget

use anyhow::{bail, Result};
use tracing::info;

use crate::budget::BudgetManager;
use crate::claude::ClaudeClient;
use crate::openai::OpenAiClient;
use crate::proto::api_gateway::ApiInferRequest;
use crate::proto::common::InferenceResponse;

/// Routes API requests to the appropriate provider
pub struct RequestRouter {
    /// Cache of recent responses (prompt hash → response)
    cache: std::collections::HashMap<u64, CachedResponse>,
    cache_max_entries: usize,
}

struct CachedResponse {
    response: InferenceResponse,
    cached_at: i64,
    ttl_seconds: i64,
}

impl RequestRouter {
    pub fn new() -> Self {
        Self {
            cache: std::collections::HashMap::new(),
            cache_max_entries: 1000,
        }
    }

    /// Route a request to the best available provider
    pub async fn route_request(
        &mut self,
        request: &ApiInferRequest,
        claude: &ClaudeClient,
        openai: &OpenAiClient,
        qwen3: &OpenAiClient,
        local: &OpenAiClient,
        budget: &mut BudgetManager,
    ) -> Result<InferenceResponse> {
        // Check cache
        let cache_key = hash_request(&request.prompt, &request.system_prompt);
        if let Some(cached) = self.get_cached(cache_key) {
            info!("Cache hit for request");
            return Ok(cached);
        }

        // Select provider
        let provider = self.select_provider(request, claude, openai, qwen3, local, budget);

        // Build fallback chain based on what's available.
        // "local" is always the final fallback (always available, no API key needed).
        let fallback_order: Vec<&str> = match provider.as_str() {
            "claude" => vec!["openai", "qwen3", "local"],
            "openai" => vec!["claude", "qwen3", "local"],
            "qwen3" => vec!["claude", "openai", "local"],
            "local" => vec!["qwen3", "claude", "openai"],
            _ => vec!["local"],
        };

        // Try primary provider
        let response = self
            .try_provider(&provider, request, claude, openai, qwen3, local, budget)
            .await;

        let response = match response {
            Ok(r) => Ok(r),
            Err(e) if request.allow_fallback => {
                info!("{provider} failed: {e}, trying fallbacks...");
                let mut last_err = e;
                let mut success = None;
                for fb in &fallback_order {
                    match self
                        .try_provider(fb, request, claude, openai, qwen3, local, budget)
                        .await
                    {
                        Ok(r) => {
                            info!("Fallback to {fb} succeeded");
                            success = Some(r);
                            break;
                        }
                        Err(e) => {
                            info!("Fallback {fb} also failed: {e}");
                            last_err = e;
                        }
                    }
                }
                success.ok_or(last_err)
            }
            Err(e) => Err(e),
        }?;

        // Cache the response
        self.cache_response(cache_key, &response);

        Ok(response)
    }

    /// Try a single provider
    async fn try_provider(
        &self,
        provider: &str,
        request: &ApiInferRequest,
        claude: &ClaudeClient,
        openai: &OpenAiClient,
        qwen3: &OpenAiClient,
        local: &OpenAiClient,
        budget: &mut BudgetManager,
    ) -> Result<InferenceResponse> {
        match provider {
            "claude" => {
                if !claude.is_available() {
                    bail!("Claude API key not configured");
                }
                let r = claude
                    .infer(
                        &request.prompt,
                        &request.system_prompt,
                        request.max_tokens,
                        request.temperature,
                    )
                    .await?;
                budget.record_usage("claude", r.tokens_used, &r.model_used);
                Ok(r)
            }
            "openai" => {
                if !openai.is_available() {
                    bail!("OpenAI API key not configured");
                }
                let r = openai
                    .infer(
                        &request.prompt,
                        &request.system_prompt,
                        request.max_tokens,
                        request.temperature,
                    )
                    .await?;
                budget.record_usage("openai", r.tokens_used, &r.model_used);
                Ok(r)
            }
            "qwen3" => {
                if !qwen3.is_available() {
                    bail!("Qwen3 API key not configured");
                }
                let r = qwen3
                    .infer(
                        &request.prompt,
                        &request.system_prompt,
                        request.max_tokens,
                        request.temperature,
                    )
                    .await?;
                budget.record_usage("qwen3", r.tokens_used, &r.model_used);
                Ok(r)
            }
            "local" => {
                // Local LLM is always "available" — it uses a placeholder API key.
                // If the local llama-server is down, the HTTP call will fail and
                // the fallback chain will try other providers.
                let r = local
                    .infer(
                        &request.prompt,
                        &request.system_prompt,
                        request.max_tokens,
                        request.temperature,
                    )
                    .await?;
                budget.record_usage("local", r.tokens_used, &r.model_used);
                Ok(r)
            }
            _ => bail!("Unknown provider: {provider}"),
        }
    }

    /// Select the best provider for a request.
    /// Falls back to "local" if no API keys are configured.
    pub fn select_provider(
        &self,
        request: &ApiInferRequest,
        claude: &ClaudeClient,
        openai: &OpenAiClient,
        qwen3: &OpenAiClient,
        _local: &OpenAiClient,
        budget: &BudgetManager,
    ) -> String {
        // Prefer explicitly requested provider
        if !request.preferred_provider.is_empty() {
            return request.preferred_provider.clone();
        }

        // Priority: Claude > OpenAI > Qwen3 > Local (by capability)
        if claude.is_available() && !budget.is_provider_budget_exceeded("claude") {
            "claude".to_string()
        } else if openai.is_available() && !budget.is_provider_budget_exceeded("openai") {
            "openai".to_string()
        } else if qwen3.is_available() && !budget.is_provider_budget_exceeded("qwen3") {
            "qwen3".to_string()
        } else {
            // Local LLM is always available as final fallback (no API key needed)
            "local".to_string()
        }
    }

    fn get_cached(&self, key: u64) -> Option<InferenceResponse> {
        let now = chrono::Utc::now().timestamp();
        self.cache.get(&key).and_then(|cached| {
            if now - cached.cached_at < cached.ttl_seconds {
                Some(cached.response.clone())
            } else {
                None
            }
        })
    }

    fn cache_response(&mut self, key: u64, response: &InferenceResponse) {
        if self.cache.len() >= self.cache_max_entries {
            // Remove oldest entry
            let oldest = self
                .cache
                .iter()
                .min_by_key(|(_, v)| v.cached_at)
                .map(|(k, _)| *k);
            if let Some(k) = oldest {
                self.cache.remove(&k);
            }
        }

        self.cache.insert(
            key,
            CachedResponse {
                response: response.clone(),
                cached_at: chrono::Utc::now().timestamp(),
                ttl_seconds: 3600, // 1 hour default TTL
            },
        );
    }
}

/// Simple hash for cache keys
fn hash_request(prompt: &str, system_prompt: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    prompt.hash(&mut hasher);
    system_prompt.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(prompt: &str, preferred: &str, allow_fallback: bool) -> ApiInferRequest {
        ApiInferRequest {
            prompt: prompt.to_string(),
            system_prompt: String::new(),
            max_tokens: 100,
            temperature: 0.3,
            preferred_provider: preferred.to_string(),
            requesting_agent: "test-agent".into(),
            task_id: "task-1".into(),
            allow_fallback,
        }
    }

    fn make_clients() -> (ClaudeClient, OpenAiClient, OpenAiClient, OpenAiClient) {
        let claude = ClaudeClient::new("test-claude-key".into());
        let openai = OpenAiClient::with_config(
            "test-openai-key".into(),
            "https://api.openai.com".into(),
            "gpt-5".into(),
        );
        let qwen3 = OpenAiClient::with_config(
            "test-qwen3-key".into(),
            "https://api.viwoapp.net".into(),
            "qwen3:30b-128k".into(),
        );
        let local = OpenAiClient::with_config(
            "local-no-key-needed".into(),
            "http://127.0.0.1:8082".into(),
            "local".into(),
        );
        (claude, openai, qwen3, local)
    }

    #[test]
    fn test_select_provider_preferred_openai() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(100.0, 50.0);
        let (claude, openai, qwen3, local) = make_clients();
        let request = make_request("hello", "openai", false);

        let provider = router.select_provider(&request, &claude, &openai, &qwen3, &local, &budget);
        assert_eq!(provider, "openai");
    }

    #[test]
    fn test_select_provider_preferred_claude() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(100.0, 50.0);
        let (claude, openai, qwen3, local) = make_clients();
        let request = make_request("hello", "claude", false);

        let provider = router.select_provider(&request, &claude, &openai, &qwen3, &local, &budget);
        assert_eq!(provider, "claude");
    }

    #[test]
    fn test_select_provider_preferred_qwen3() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(100.0, 50.0);
        let (claude, openai, qwen3, local) = make_clients();
        let request = make_request("hello", "qwen3", false);

        let provider = router.select_provider(&request, &claude, &openai, &qwen3, &local, &budget);
        assert_eq!(provider, "qwen3");
    }

    #[test]
    fn test_select_provider_fallback_to_local() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(100.0, 50.0);
        // All API clients with empty keys (unavailable)
        let claude = ClaudeClient::new(String::new());
        let openai = OpenAiClient::with_config(String::new(), "https://api.openai.com".into(), "gpt-5".into());
        let qwen3 = OpenAiClient::with_config(String::new(), "https://api.viwoapp.net".into(), "qwen3:30b-128k".into());
        let local = OpenAiClient::with_config("local-no-key-needed".into(), "http://127.0.0.1:8082".into(), "local".into());
        let request = make_request("hello", "", false);

        let provider = router.select_provider(&request, &claude, &openai, &qwen3, &local, &budget);
        assert_eq!(provider, "local", "Should fall back to local when no API keys configured");
    }

    #[test]
    fn test_hash_request_deterministic() {
        let hash1 = hash_request("prompt1", "system1");
        let hash2 = hash_request("prompt1", "system1");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_request_different_prompts() {
        let hash1 = hash_request("prompt1", "system1");
        let hash2 = hash_request("prompt2", "system1");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_request_different_system_prompts() {
        let hash1 = hash_request("prompt1", "system1");
        let hash2 = hash_request("prompt1", "system2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_cache_response_and_retrieve() {
        let mut router = RequestRouter::new();
        let key = hash_request("test prompt", "system");

        let response = InferenceResponse {
            text: "cached response".into(),
            tokens_used: 100,
            latency_ms: 50,
            model_used: "test-model".into(),
            intelligence_level: "strategic".into(),
        };

        router.cache_response(key, &response);

        let cached = router.get_cached(key);
        assert!(cached.is_some());
        let cached = cached.unwrap();
        assert_eq!(cached.text, "cached response");
        assert_eq!(cached.tokens_used, 100);
        assert_eq!(cached.model_used, "test-model");
    }

    #[test]
    fn test_cache_miss() {
        let router = RequestRouter::new();
        let key = hash_request("uncached", "prompt");

        let cached = router.get_cached(key);
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_eviction_at_capacity() {
        let mut router = RequestRouter::new();
        // Fill cache to max
        for i in 0..router.cache_max_entries + 10 {
            let key = hash_request(&format!("prompt_{i}"), "sys");
            let response = InferenceResponse {
                text: format!("response_{i}"),
                tokens_used: 10,
                latency_ms: 5,
                model_used: "test".into(),
                intelligence_level: "tactical".into(),
            };
            router.cache_response(key, &response);
        }

        // Cache should not exceed max_entries
        assert!(router.cache.len() <= router.cache_max_entries);
    }

    #[test]
    fn test_new_router_empty_cache() {
        let router = RequestRouter::new();
        assert!(router.cache.is_empty());
        assert_eq!(router.cache_max_entries, 1000);
    }
}
