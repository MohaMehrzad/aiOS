//! aiOS AI Runtime — local model management via llama.cpp
//!
//! Exposes a gRPC interface on port 50055 that lets other aiOS services:
//!   - Load / unload GGUF models (spawns llama-server processes)
//!   - Run single-shot or streaming inference
//!   - Query model health and availability
//!
//! Each loaded model is backed by an independent `llama-server` process
//! communicating over the OpenAI-compatible HTTP API on a per-model port.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tonic::transport::Server;
use tracing::{error, info};

mod grpc_service;
mod inference;
mod model_manager;

pub mod proto {
    pub mod runtime {
        tonic::include_proto!("aios.runtime");
    }
    pub mod common {
        tonic::include_proto!("aios.common");
    }
}

use grpc_service::AIRuntimeService;
use inference::InferenceEngine;
use model_manager::ModelManager;
use proto::runtime::ai_runtime_server::AiRuntimeServer;

/// Interval between background health checks of managed models.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .compact()
        .init();

    info!("aiOS AI Runtime starting...");

    let model_manager = Arc::new(Mutex::new(ModelManager::new()));
    let inference_engine = Arc::new(InferenceEngine::new());
    let start_time = Instant::now();

    // Spawn background health-check task.
    let health_mgr = Arc::clone(&model_manager);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEALTH_CHECK_INTERVAL);
        loop {
            interval.tick().await;
            let mut mgr = health_mgr.lock().await;
            mgr.health_check_all().await;
        }
    });

    // Auto-load models found in the model directory
    {
        let mut mgr = model_manager.lock().await;
        let model_dir = std::env::var("AIOS_MODEL_DIR")
            .unwrap_or_else(|_| "/var/lib/aios/models/".to_string());
        let model_path = std::path::Path::new(&model_dir);

        if model_path.exists() {
            info!("Scanning {model_dir} for GGUF models to auto-load...");
            if let Ok(entries) = std::fs::read_dir(model_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("gguf") {
                        let file_name = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();

                        // Choose context length and threads based on model size
                        let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        let (ctx, threads) = if file_size > 2_000_000_000 {
                            // Large model (>2GB) — Mistral 7B class
                            (4096_i32, 6_i32)
                        } else {
                            // Small model — TinyLlama class
                            (2048_i32, 4_i32)
                        };

                        info!(
                            model = %file_name,
                            path = %path.display(),
                            size_mb = file_size / 1_000_000,
                            ctx,
                            "Auto-loading model"
                        );

                        let req = crate::proto::runtime::LoadModelRequest {
                            model_name: file_name.clone(),
                            model_path: path.to_string_lossy().to_string(),
                            context_length: ctx,
                            gpu_layers: 0,
                            threads,
                            port: 0,
                        };

                        match mgr.load_model(req).await {
                            Ok(status) => info!(
                                model = %file_name,
                                status = %status.status,
                                port = status.port,
                                "Model auto-loaded"
                            ),
                            Err(e) => error!(model = %file_name, "Failed to auto-load: {e:#}"),
                        }
                    }
                }
            }
        } else {
            info!("Model directory {model_dir} not found, skipping auto-load");
        }
    }

    let service = AIRuntimeService {
        model_manager,
        inference_engine,
        start_time,
    };

    let addr = "[::]:50055".parse().context("Invalid listen address")?;
    info!("AI Runtime gRPC server listening on {addr}");

    // Graceful shutdown on SIGTERM.
    let shutdown = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => info!("Received SIGINT, shutting down..."),
            Err(e) => error!("Failed to listen for SIGINT: {e}"),
        }

        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                tokio::select! {
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM, shutting down...");
                    }
                    () = std::future::pending::<()>() => {}
                }
            }
        }
    };

    Server::builder()
        .add_service(AiRuntimeServer::new(service))
        .serve_with_shutdown(addr, shutdown)
        .await
        .context("AI Runtime gRPC server failed")?;

    info!("aiOS AI Runtime shut down cleanly");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_interval() {
        assert_eq!(HEALTH_CHECK_INTERVAL, Duration::from_secs(10));
    }

    #[test]
    fn test_proto_module_accessible() {
        // Verify the proto modules compile and the expected types exist.
        let _empty = proto::common::Empty {};
        let _req = proto::runtime::InferRequest {
            model: String::new(),
            prompt: String::new(),
            system_prompt: String::new(),
            max_tokens: 0,
            temperature: 0.0,
            intelligence_level: String::new(),
            requesting_agent: String::new(),
            task_id: String::new(),
        };
    }

    #[test]
    fn test_listen_address_parses() {
        let addr: std::net::SocketAddr = "[::]:50055".parse().unwrap();
        assert_eq!(addr.port(), 50055);
    }
}
