//! aiOS API Gateway — External AI API integration
//!
//! Provides gRPC interface to Claude and OpenAI APIs with:
//! - Provider routing and fallback
//! - Budget management and cost tracking
//! - Response caching
//! - Rate limiting

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Server;
use tracing::info;

mod budget;
mod claude;
mod openai;
mod router;

pub mod proto {
    pub mod common {
        tonic::include_proto!("aios.common");
    }
    pub mod api_gateway {
        tonic::include_proto!("aios.api_gateway");
    }
}

use proto::api_gateway::api_gateway_server::{ApiGateway, ApiGatewayServer};

/// Shared gateway state
pub struct GatewayState {
    pub claude_client: claude::ClaudeClient,
    pub openai_client: openai::OpenAiClient,
    pub qwen3_client: openai::OpenAiClient,
    pub request_router: router::RequestRouter,
    pub budget_manager: budget::BudgetManager,
}

/// gRPC service implementation
pub struct ApiGatewayService {
    state: Arc<RwLock<GatewayState>>,
}

#[tonic::async_trait]
impl ApiGateway for ApiGatewayService {
    async fn infer(
        &self,
        request: tonic::Request<proto::api_gateway::ApiInferRequest>,
    ) -> Result<tonic::Response<proto::common::InferenceResponse>, tonic::Status> {
        let req = request.into_inner();
        info!(
            "API inference request: provider={}, agent={}, task={}",
            req.preferred_provider, req.requesting_agent, req.task_id
        );

        let mut state = self.state.write().await;

        // Check budget
        if state.budget_manager.is_budget_exceeded() {
            return Err(tonic::Status::resource_exhausted("API budget exceeded"));
        }

        // Destructure to satisfy the borrow checker — each field is borrowed independently
        let GatewayState {
            ref claude_client,
            ref openai_client,
            ref qwen3_client,
            ref mut request_router,
            ref mut budget_manager,
        } = *state;

        // Route request to appropriate provider
        let response = request_router
            .route_request(
                &req,
                claude_client,
                openai_client,
                qwen3_client,
                budget_manager,
            )
            .await
            .map_err(|e| tonic::Status::internal(format!("API request failed: {e}")))?;

        Ok(tonic::Response::new(response))
    }

    type StreamInferStream = tokio_stream::wrappers::ReceiverStream<
        Result<proto::api_gateway::StreamChunk, tonic::Status>,
    >;

    async fn stream_infer(
        &self,
        request: tonic::Request<proto::api_gateway::ApiInferRequest>,
    ) -> Result<tonic::Response<Self::StreamInferStream>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(128);

        tokio::spawn(async move {
            let state = state.write().await;

            let provider = state.request_router.select_provider(
                &req,
                &state.claude_client,
                &state.openai_client,
                &state.qwen3_client,
                &state.budget_manager,
            );

            let result = match provider.as_str() {
                "claude" => {
                    state
                        .claude_client
                        .infer(
                            &req.prompt,
                            &req.system_prompt,
                            req.max_tokens,
                            req.temperature,
                        )
                        .await
                }
                "openai" => {
                    state
                        .openai_client
                        .infer(
                            &req.prompt,
                            &req.system_prompt,
                            req.max_tokens,
                            req.temperature,
                        )
                        .await
                }
                "qwen3" => {
                    state
                        .qwen3_client
                        .infer(
                            &req.prompt,
                            &req.system_prompt,
                            req.max_tokens,
                            req.temperature,
                        )
                        .await
                }
                _ => Err(anyhow::anyhow!("No available provider")),
            };

            match result {
                Ok(response) => {
                    // Send as single chunk (streaming will be implemented per-provider later)
                    let _ = tx
                        .send(Ok(proto::api_gateway::StreamChunk {
                            text: response.text,
                            done: true,
                            provider,
                        }))
                        .await;
                }
                Err(e) => {
                    let _ = tx.send(Err(tonic::Status::internal(e.to_string()))).await;
                }
            }
        });

        Ok(tonic::Response::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
    }

    async fn get_budget(
        &self,
        _request: tonic::Request<proto::common::Empty>,
    ) -> Result<tonic::Response<proto::api_gateway::BudgetStatus>, tonic::Status> {
        let state = self.state.read().await;
        let status = state.budget_manager.get_status();
        Ok(tonic::Response::new(status))
    }

    async fn get_usage(
        &self,
        request: tonic::Request<proto::api_gateway::UsageRequest>,
    ) -> Result<tonic::Response<proto::api_gateway::UsageResponse>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let usage = state.budget_manager.get_usage(&req.provider, req.days);
        Ok(tonic::Response::new(usage))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .compact()
        .init();

    info!("aiOS API Gateway starting...");

    // Load API keys from environment (set by aios-init from kernel keyring)
    let claude_key = std::env::var("CLAUDE_API_KEY").unwrap_or_default();
    let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let qwen3_key = std::env::var("QWEN3_API_KEY").unwrap_or_default();

    // Qwen3 config
    let qwen3_base_url =
        std::env::var("QWEN3_BASE_URL").unwrap_or_else(|_| "https://api.viwoapp.net".to_string());
    let qwen3_model = std::env::var("QWEN3_MODEL").unwrap_or_else(|_| "qwen3:30b-128k".to_string());

    // OpenAI config
    let openai_model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5".to_string());

    let available: Vec<&str> = [
        if !claude_key.is_empty() {
            Some("claude")
        } else {
            None
        },
        if !openai_key.is_empty() {
            Some("openai")
        } else {
            None
        },
        if !qwen3_key.is_empty() {
            Some("qwen3")
        } else {
            None
        },
    ]
    .iter()
    .filter_map(|x| *x)
    .collect();

    if available.is_empty() {
        tracing::warn!("No API keys configured — API gateway will reject all requests");
    } else {
        info!("Available providers: {}", available.join(", "));
    }

    let state = Arc::new(RwLock::new(GatewayState {
        claude_client: claude::ClaudeClient::new(claude_key),
        openai_client: openai::OpenAiClient::with_config(
            openai_key,
            "https://api.openai.com".to_string(),
            openai_model,
        ),
        qwen3_client: openai::OpenAiClient::with_config(qwen3_key, qwen3_base_url, qwen3_model),
        request_router: router::RequestRouter::new(),
        budget_manager: budget::BudgetManager::new(100.0, 50.0),
    }));

    let service = ApiGatewayService { state };

    let addr: SocketAddr = "0.0.0.0:50054".parse()?;
    info!("API Gateway gRPC server listening on {addr}");

    Server::builder()
        .add_service(ApiGatewayServer::new(service))
        .serve(addr)
        .await
        .context("API Gateway gRPC server failed")?;

    Ok(())
}
