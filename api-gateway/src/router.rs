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
        budget: &mut BudgetManager,
    ) -> Result<InferenceResponse> {
        // Check cache
        let cache_key = hash_request(&request.prompt, &request.system_prompt);
        if let Some(cached) = self.get_cached(cache_key) {
            info!("Cache hit for request");
            return Ok(cached);
        }

        // Select provider
        let provider = self.select_provider(request, budget);

        // Execute request
        let response = match provider.as_str() {
            "claude" => {
                let resp = claude
                    .infer(
                        &request.prompt,
                        &request.system_prompt,
                        request.max_tokens,
                        request.temperature,
                    )
                    .await;

                match resp {
                    Ok(r) => {
                        budget.record_usage("claude", r.tokens_used, &r.model_used);
                        Ok(r)
                    }
                    Err(e) if request.allow_fallback && openai.is_available() => {
                        info!("Claude failed, falling back to OpenAI: {e}");
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
                    Err(e) => Err(e),
                }
            }
            "openai" => {
                let resp = openai
                    .infer(
                        &request.prompt,
                        &request.system_prompt,
                        request.max_tokens,
                        request.temperature,
                    )
                    .await;

                match resp {
                    Ok(r) => {
                        budget.record_usage("openai", r.tokens_used, &r.model_used);
                        Ok(r)
                    }
                    Err(e) if request.allow_fallback && claude.is_available() => {
                        info!("OpenAI failed, falling back to Claude: {e}");
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
                    Err(e) => Err(e),
                }
            }
            _ => bail!("No available API provider"),
        }?;

        // Cache the response
        self.cache_response(cache_key, &response);

        Ok(response)
    }

    /// Select the best provider for a request
    pub fn select_provider(&self, request: &ApiInferRequest, budget: &BudgetManager) -> String {
        // Prefer explicitly requested provider
        if !request.preferred_provider.is_empty() {
            return request.preferred_provider.clone();
        }

        // Claude is primary, OpenAI is fallback
        if !budget.is_provider_budget_exceeded("claude") {
            "claude".to_string()
        } else if !budget.is_provider_budget_exceeded("openai") {
            "openai".to_string()
        } else {
            "none".to_string()
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

    fn make_request(
        prompt: &str,
        preferred: &str,
        allow_fallback: bool,
    ) -> ApiInferRequest {
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

    #[test]
    fn test_select_provider_default_claude() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(100.0, 50.0);
        let request = make_request("hello", "", false);

        let provider = router.select_provider(&request, &budget);
        assert_eq!(provider, "claude");
    }

    #[test]
    fn test_select_provider_preferred_openai() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(100.0, 50.0);
        let request = make_request("hello", "openai", false);

        let provider = router.select_provider(&request, &budget);
        assert_eq!(provider, "openai");
    }

    #[test]
    fn test_select_provider_preferred_claude() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(100.0, 50.0);
        let request = make_request("hello", "claude", false);

        let provider = router.select_provider(&request, &budget);
        assert_eq!(provider, "claude");
    }

    #[test]
    fn test_select_provider_claude_exceeded_fallback_openai() {
        let router = RequestRouter::new();
        // Set a very small claude budget that will be exceeded
        let budget = BudgetManager::new(0.0, 50.0);
        // Claude budget is 0, so it's immediately exceeded
        let request = make_request("hello", "", false);

        let provider = router.select_provider(&request, &budget);
        assert_eq!(provider, "openai");
    }

    #[test]
    fn test_select_provider_both_exceeded() {
        let router = RequestRouter::new();
        let budget = BudgetManager::new(0.0, 0.0); // Both zero budgets

        let request = make_request("hello", "", false);
        let provider = router.select_provider(&request, &budget);
        assert_eq!(provider, "none");
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
