//! gRPC service implementation for the AIRuntime service.
//!
//! Wires the tonic-generated trait to [`ModelManager`] and [`InferenceEngine`].

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

use crate::inference::InferenceEngine;
use crate::model_manager::ModelManager;
use crate::proto::common::{Empty, HealthStatus, Status as ProtoStatus};
use crate::proto::runtime::ai_runtime_server::AiRuntime;
use crate::proto::runtime::{
    InferChunk, InferRequest, InferResponse, LoadModelRequest, ModelList, ModelStatus,
    UnloadModelRequest,
};

/// Shared gRPC service implementation.
pub struct AIRuntimeService {
    pub model_manager: Arc<Mutex<ModelManager>>,
    pub inference_engine: Arc<InferenceEngine>,
    pub start_time: Instant,
}

#[tonic::async_trait]
impl AiRuntime for AIRuntimeService {
    // ------------------------------------------------------------------
    // LoadModel
    // ------------------------------------------------------------------
    async fn load_model(
        &self,
        request: Request<LoadModelRequest>,
    ) -> Result<Response<ModelStatus>, Status> {
        let req = request.into_inner();
        info!(model = %req.model_name, "gRPC LoadModel");

        let mut mgr = self.model_manager.lock().await;
        match mgr.load_model(req).await {
            Ok(status) => Ok(Response::new(status)),
            Err(e) => {
                error!("LoadModel failed: {e:#}");
                Err(Status::internal(format!("Failed to load model: {e:#}")))
            }
        }
    }

    // ------------------------------------------------------------------
    // UnloadModel
    // ------------------------------------------------------------------
    async fn unload_model(
        &self,
        request: Request<UnloadModelRequest>,
    ) -> Result<Response<ProtoStatus>, Status> {
        let req = request.into_inner();
        info!(model = %req.model_name, "gRPC UnloadModel");

        let mut mgr = self.model_manager.lock().await;
        match mgr.unload_model(&req.model_name).await {
            Ok(()) => Ok(Response::new(ProtoStatus {
                success: true,
                message: format!("Model '{}' unloaded", req.model_name),
            })),
            Err(e) => {
                error!("UnloadModel failed: {e:#}");
                Err(Status::internal(format!("Failed to unload model: {e:#}")))
            }
        }
    }

    // ------------------------------------------------------------------
    // ListModels
    // ------------------------------------------------------------------
    async fn list_models(&self, _request: Request<Empty>) -> Result<Response<ModelList>, Status> {
        let mgr = self.model_manager.lock().await;
        let models = mgr.list_models();
        info!(count = models.len(), "gRPC ListModels");
        Ok(Response::new(ModelList { models }))
    }

    // ------------------------------------------------------------------
    // Infer (unary)
    // ------------------------------------------------------------------
    async fn infer(
        &self,
        request: Request<InferRequest>,
    ) -> Result<Response<InferResponse>, Status> {
        let req = request.into_inner();
        info!(
            model = %req.model,
            level = %req.intelligence_level,
            agent = %req.requesting_agent,
            task = %req.task_id,
            "gRPC Infer"
        );

        let (port, model_name) = self.resolve_model(&req).await?;

        match self.inference_engine.infer(port, &model_name, &req).await {
            Ok(resp) => Ok(Response::new(resp)),
            Err(e) => {
                error!(model = %model_name, "Inference failed: {e:#}");
                Err(Status::internal(format!("Inference failed: {e:#}")))
            }
        }
    }

    // ------------------------------------------------------------------
    // StreamInfer (server-streaming)
    // ------------------------------------------------------------------
    type StreamInferStream = tokio_stream::wrappers::ReceiverStream<Result<InferChunk, Status>>;

    async fn stream_infer(
        &self,
        request: Request<InferRequest>,
    ) -> Result<Response<Self::StreamInferStream>, Status> {
        let req = request.into_inner();
        info!(
            model = %req.model,
            level = %req.intelligence_level,
            agent = %req.requesting_agent,
            task = %req.task_id,
            "gRPC StreamInfer"
        );

        let (port, model_name) = self.resolve_model(&req).await?;

        match self
            .inference_engine
            .stream_infer(port, &model_name, &req)
            .await
        {
            Ok(stream) => Ok(Response::new(stream)),
            Err(e) => {
                error!(model = %model_name, "Stream inference failed: {e:#}");
                Err(Status::internal(format!("Stream inference failed: {e:#}")))
            }
        }
    }

    // ------------------------------------------------------------------
    // HealthCheck
    // ------------------------------------------------------------------
    async fn health_check(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<HealthStatus>, Status> {
        let mgr = self.model_manager.lock().await;
        let models = mgr.list_models();
        let loaded_count = models.iter().filter(|m| m.status == "ready").count();
        let total_count = models.len();

        let uptime = self.start_time.elapsed().as_secs() as i64;

        let mut details = std::collections::HashMap::new();
        details.insert("loaded_models".to_string(), loaded_count.to_string());
        details.insert("total_models".to_string(), total_count.to_string());

        for m in &models {
            details.insert(
                format!("model:{}", m.model_name),
                format!("status={} port={}", m.status, m.port),
            );
        }

        info!(loaded_count, total_count, uptime, "gRPC HealthCheck");

        Ok(Response::new(HealthStatus {
            healthy: true,
            service: "aios-runtime".to_string(),
            message: format!("{loaded_count}/{total_count} models loaded, uptime {uptime}s"),
            uptime_seconds: uptime,
            details,
        }))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl AIRuntimeService {
    /// Resolve the target model from the request.  Tries the explicit model
    /// name first, then falls back to intelligence-level routing.
    async fn resolve_model(&self, req: &InferRequest) -> Result<(u16, String), Status> {
        let mut mgr = self.model_manager.lock().await;

        // 1. Explicit model name.
        if !req.model.is_empty() {
            if let Some(port) = mgr.model_port(&req.model) {
                return Ok((port, req.model.clone()));
            }
            warn!(model = %req.model, "Requested model not ready, trying level routing");
        }

        // 2. Intelligence-level routing.
        if !req.intelligence_level.is_empty() {
            if let Some(name) = mgr.select_model_for_level(&req.intelligence_level) {
                if let Some(port) = mgr.model_port(&name) {
                    return Ok((port, name));
                }
            }

            // "reactive" and "strategic" intentionally return None from
            // select_model_for_level.
            if req.intelligence_level == "reactive" {
                return Err(Status::invalid_argument(
                    "Reactive level does not require LLM inference — handle with heuristics",
                ));
            }
            if req.intelligence_level == "strategic" {
                return Err(Status::failed_precondition(
                    "Strategic level requires external API — route via api-gateway",
                ));
            }
        }

        // 3. Last resort: any ready model.
        let models = mgr.list_models();
        for m in &models {
            if m.status == "ready" {
                if let Some(port) = mgr.model_port(&m.model_name) {
                    return Ok((port, m.model_name.clone()));
                }
            }
        }

        Err(Status::unavailable(
            "No model available for inference.  Load a model first with LoadModel.",
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_service() -> AIRuntimeService {
        AIRuntimeService {
            model_manager: Arc::new(Mutex::new(ModelManager::new())),
            inference_engine: Arc::new(InferenceEngine::new()),
            start_time: Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_health_check_empty() {
        let svc = make_service();
        let resp = svc
            .health_check(Request::new(Empty {}))
            .await
            .expect("health check should succeed");
        let status = resp.into_inner();
        assert!(status.healthy);
        assert_eq!(status.service, "aios-runtime");
        assert!(status.details.contains_key("loaded_models"));
        assert_eq!(status.details["loaded_models"], "0");
    }

    #[tokio::test]
    async fn test_list_models_empty() {
        let svc = make_service();
        let resp = svc
            .list_models(Request::new(Empty {}))
            .await
            .expect("list should succeed");
        assert!(resp.into_inner().models.is_empty());
    }

    #[tokio::test]
    async fn test_infer_no_model() {
        let svc = make_service();
        let req = InferRequest {
            model: String::new(),
            prompt: "hello".to_string(),
            system_prompt: String::new(),
            max_tokens: 10,
            temperature: 0.5,
            intelligence_level: String::new(),
            requesting_agent: "test".to_string(),
            task_id: "t1".to_string(),
        };
        let err = svc.infer(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unavailable);
    }

    #[tokio::test]
    async fn test_infer_reactive_rejected() {
        let svc = make_service();
        let req = InferRequest {
            model: String::new(),
            prompt: "hello".to_string(),
            system_prompt: String::new(),
            max_tokens: 10,
            temperature: 0.5,
            intelligence_level: "reactive".to_string(),
            requesting_agent: "test".to_string(),
            task_id: "t1".to_string(),
        };
        let err = svc.infer(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_infer_strategic_rejected() {
        let svc = make_service();
        let req = InferRequest {
            model: String::new(),
            prompt: "hello".to_string(),
            system_prompt: String::new(),
            max_tokens: 10,
            temperature: 0.5,
            intelligence_level: "strategic".to_string(),
            requesting_agent: "test".to_string(),
            task_id: "t1".to_string(),
        };
        let err = svc.infer(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn test_unload_nonexistent() {
        let svc = make_service();
        let req = UnloadModelRequest {
            model_name: "ghost".to_string(),
        };
        let err = svc.unload_model(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Internal);
    }
}
