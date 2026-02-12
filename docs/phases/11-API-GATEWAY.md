# Phase 11: External API Gateway

## Goal
Integrate Claude and OpenAI APIs as the strategic intelligence layer, with budget management, rate limiting, fallback routing, and response caching.

## Prerequisites
- Phase 5 complete (orchestrator needs strategic layer for complex planning)
- API keys for Claude and/or OpenAI

---

## Step-by-Step

### Step 11.1: Implement API Gateway Service (Rust)

**Claude Code prompt**: "Implement the aios-api-gateway Rust service — manages connections to Claude and OpenAI APIs, handles routing, rate limiting, budget tracking, and fallback"

```
File: api-gateway/src/

api-gateway/src/
├── main.rs           — gRPC server startup
├── router.rs         — Route requests to best available API
├── claude.rs         — Claude API client (Anthropic SDK)
├── openai.rs         — OpenAI API client
├── budget.rs         — Cost tracking and budget enforcement
├── cache.rs          — Response caching for repeated queries
├── rate_limiter.rs   — Per-API rate limiting
└── fallback.rs       — Fallback logic when primary API unavailable
```

### gRPC API

```protobuf
// agent-core/proto/api_gateway.proto
syntax = "proto3";
package aios.api_gateway;

service ApiGateway {
    rpc Infer(ApiInferRequest) returns (ApiInferResponse);
    rpc StreamInfer(ApiInferRequest) returns (stream ApiInferChunk);
    rpc GetBudget(Empty) returns (BudgetStatus);
    rpc GetUsage(UsageRequest) returns (UsageReport);
}

message ApiInferRequest {
    string prompt = 1;
    string system_prompt = 2;
    int32 max_tokens = 3;
    float temperature = 4;
    string preferred_provider = 5;  // "claude", "openai", "auto"
    string task_type = 6;           // "planning", "coding", "analysis", "security"
    repeated Tool tools = 7;        // Tool definitions for tool use
}

message ApiInferResponse {
    string text = 1;
    string provider_used = 2;
    string model_used = 3;
    int32 input_tokens = 4;
    int32 output_tokens = 5;
    float cost_usd = 6;
    int64 latency_ms = 7;
    bool from_cache = 8;
}
```

### Step 11.2: Implement Claude API Client

**Claude Code prompt**: "Implement the Claude API client using the Anthropic API — supports messages, tool use, and streaming"

```rust
// api-gateway/src/claude.rs

pub struct ClaudeClient {
    api_key: String,
    model: String,           // claude-sonnet-4-5-20250929
    http_client: reqwest::Client,
    base_url: String,
}

impl ClaudeClient {
    pub async fn infer(&self, request: &ApiInferRequest) -> Result<ApiInferResponse> {
        let body = json!({
            "model": &self.model,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "system": request.system_prompt,
            "messages": [
                {"role": "user", "content": request.prompt}
            ]
        });

        let response = self.http_client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let data: ClaudeResponse = response.json().await?;

        Ok(ApiInferResponse {
            text: data.content[0].text.clone(),
            provider_used: "claude".to_string(),
            model_used: self.model.clone(),
            input_tokens: data.usage.input_tokens,
            output_tokens: data.usage.output_tokens,
            cost_usd: self.calculate_cost(data.usage.input_tokens, data.usage.output_tokens),
            ..Default::default()
        })
    }

    fn calculate_cost(&self, input_tokens: i32, output_tokens: i32) -> f32 {
        // Claude Sonnet pricing (update as needed)
        let input_cost = input_tokens as f32 * 3.0 / 1_000_000.0;
        let output_cost = output_tokens as f32 * 15.0 / 1_000_000.0;
        input_cost + output_cost
    }
}
```

### Step 11.3: Implement OpenAI API Client

**Claude Code prompt**: "Implement the OpenAI API client as a fallback — same interface as Claude client"

### Step 11.4: Implement Budget Manager

**Claude Code prompt**: "Implement budget tracking — monthly budgets per provider, cost logging, budget alerts, and automatic downgrade to local models when budget is low"

```rust
// api-gateway/src/budget.rs

pub struct BudgetManager {
    db: Connection,
    budgets: HashMap<String, MonthlyBudget>,
}

pub struct MonthlyBudget {
    provider: String,
    limit_usd: f64,
    spent_usd: f64,
    month: String,       // "2024-01"
    alert_threshold: f64, // 0.8 = alert at 80%
}

impl BudgetManager {
    pub fn can_spend(&self, provider: &str, estimated_cost: f64) -> BudgetDecision {
        let budget = &self.budgets[provider];
        let remaining = budget.limit_usd - budget.spent_usd;

        if estimated_cost > remaining {
            BudgetDecision::Denied { reason: "Monthly budget exhausted" }
        } else if budget.spent_usd / budget.limit_usd > budget.alert_threshold {
            BudgetDecision::AllowedWithWarning {
                remaining,
                warning: "Approaching monthly budget limit"
            }
        } else {
            BudgetDecision::Allowed { remaining }
        }
    }

    pub fn record_spend(&mut self, provider: &str, cost: f64, details: &SpendDetails) {
        // Update running total
        // Log to database
        // Check if alert threshold crossed
    }
}
```

### Step 11.5: Implement Request Router

**Claude Code prompt**: "Implement the smart request router — routes to the best API based on availability, budget, task type, and latency"

```rust
// api-gateway/src/router.rs

pub async fn route_request(request: &ApiInferRequest) -> Result<ApiInferResponse> {
    // 1. Check cache first
    if let Some(cached) = cache.lookup(request).await {
        return Ok(cached.with_from_cache(true));
    }

    // 2. Determine provider preference
    let providers = match request.preferred_provider.as_str() {
        "claude" => vec!["claude", "openai"],
        "openai" => vec!["openai", "claude"],
        _ => {
            // Auto: choose based on task type
            match request.task_type.as_str() {
                "coding" | "planning" => vec!["claude", "openai"],
                "analysis" => vec!["claude", "openai"],
                _ => vec!["claude", "openai"],
            }
        }
    };

    // 3. Try each provider in order
    for provider in providers {
        // Check budget
        let estimated_cost = estimate_cost(provider, request);
        if !budget_manager.can_spend(provider, estimated_cost).is_allowed() {
            continue;
        }

        // Check rate limit
        if !rate_limiter.allow(provider) {
            continue;
        }

        // Make the call
        match call_provider(provider, request).await {
            Ok(response) => {
                // Record spend
                budget_manager.record_spend(provider, response.cost_usd, &details);

                // Cache response
                cache.store(request, &response).await;

                return Ok(response);
            }
            Err(e) => {
                // Log failure, try next provider
                tracing::warn!("Provider {} failed: {}", provider, e);
                continue;
            }
        }
    }

    Err(anyhow!("All API providers unavailable or over budget"))
}
```

### Step 11.6: Implement Response Cache

**Claude Code prompt**: "Implement response caching — cache identical requests to avoid redundant API calls, with configurable TTL"

Caching rules:
- Cache by hash of (system_prompt + prompt + model + temperature)
- TTL: 1 hour for general queries, 24 hours for documentation queries
- Never cache security-related queries
- Max cache size: 1000 entries (LRU eviction)

### Step 11.7: Integrate with Orchestrator

**Claude Code prompt**: "Update the orchestrator's task planner and intelligence router to use the API gateway for strategic-level decisions"

### Step 11.8: Integration Test

**Claude Code prompt**: "Test: submit a complex goal that requires Claude API for planning, verify the API gateway routes correctly, tracks costs, and returns a valid plan"

---

## Deliverables Checklist

- [ ] API Gateway gRPC service starts
- [ ] Claude API client works (send prompt → get response)
- [ ] OpenAI API client works as fallback
- [ ] Budget tracking records all API costs
- [ ] Budget enforcement blocks over-budget requests
- [ ] Rate limiting prevents API throttling
- [ ] Response caching reduces redundant calls
- [ ] Fallback routing works (Claude down → OpenAI)
- [ ] Orchestrator uses API gateway for strategic tasks
- [ ] Cost report shows per-day and per-month spending

---

## Next Phase
→ [Phase 12: Distribution Packaging](./12-DISTRO.md)
