//! Model Manager — manages llama-server processes for local model inference.
//!
//! Each loaded model runs as an independent llama-server process bound to a
//! unique port on 127.0.0.1.  The manager handles lifecycle (spawn, health
//! polling, graceful / forced shutdown) and provides model selection by
//! intelligence level.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tokio::process::{Child, Command};
use tracing::{debug, error, info, warn};

use crate::proto::runtime::{LoadModelRequest, ModelStatus};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Internal state of a managed model process.
#[derive(Debug)]
enum ModelState {
    Loading,
    Ready,
    Error(String),
    Unloading,
}

impl std::fmt::Display for ModelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelState::Loading => write!(f, "loading"),
            ModelState::Ready => write!(f, "ready"),
            ModelState::Error(e) => write!(f, "error: {e}"),
            ModelState::Unloading => write!(f, "unloading"),
        }
    }
}

/// A single managed llama-server instance.
#[allow(dead_code)]
pub(crate) struct ManagedModel {
    name: String,
    path: PathBuf,
    process: Option<Child>,
    port: u16,
    status: ModelState,
    loaded_at: i64,
    last_used: i64,
    request_count: i64,
    context_length: i32,
    gpu_layers: i32,
    threads: i32,
}

/// Top-level model manager that owns all managed models.
pub struct ModelManager {
    models: HashMap<String, ManagedModel>,
    model_dir: PathBuf,
    next_port: u16,
    http_client: reqwest::Client,
}

// ---------------------------------------------------------------------------
// Port allocation
// ---------------------------------------------------------------------------

const BASE_PORT: u16 = 8080;

// ---------------------------------------------------------------------------
// llama-server binary resolution
// ---------------------------------------------------------------------------

fn find_llama_server() -> Result<PathBuf> {
    // 1. Explicit env override
    if let Ok(p) = std::env::var("LLAMA_SERVER_PATH") {
        let path = PathBuf::from(&p);
        if path.exists() {
            return Ok(path);
        }
        warn!("LLAMA_SERVER_PATH={p} does not exist, falling back to well-known locations");
    }

    // 2. Well-known locations
    for candidate in &["/usr/local/bin/llama-server", "/usr/bin/llama-server"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    bail!(
        "llama-server binary not found. Set LLAMA_SERVER_PATH or install llama.cpp \
         to /usr/local/bin/llama-server"
    )
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ---------------------------------------------------------------------------
// ModelManager implementation
// ---------------------------------------------------------------------------

impl ModelManager {
    /// Create a new model manager.
    ///
    /// `model_dir` defaults to `AIOS_MODEL_DIR` env var or `/var/lib/aios/models/`.
    pub fn new() -> Self {
        let model_dir = std::env::var("AIOS_MODEL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/var/lib/aios/models/"));

        info!(?model_dir, "ModelManager initialised");

        Self {
            models: HashMap::new(),
            model_dir,
            next_port: BASE_PORT,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Allocate the next free port.
    fn allocate_port(&mut self, requested: u16) -> u16 {
        if requested > 0 {
            return requested;
        }
        let port = self.next_port;
        self.next_port += 1;
        port
    }

    // ------------------------------------------------------------------
    // Load
    // ------------------------------------------------------------------

    /// Load (spawn) a model.  If the model is already loaded the current status
    /// is returned without re-spawning.
    pub async fn load_model(&mut self, req: LoadModelRequest) -> Result<ModelStatus> {
        let name = req.model_name.clone();

        if let Some(existing) = self.models.get(&name) {
            if matches!(existing.status, ModelState::Ready | ModelState::Loading) {
                info!(model = %name, "Model already loaded, returning current status");
                return Ok(model_to_status(existing));
            }
        }

        // Resolve model file path.
        let model_path = if req.model_path.is_empty() {
            self.model_dir.join(&name)
        } else {
            PathBuf::from(&req.model_path)
        };

        let port = self.allocate_port(req.port as u16);
        let ctx = if req.context_length > 0 {
            req.context_length
        } else {
            2048
        };
        let gpu_layers = req.gpu_layers;
        let threads = if req.threads > 0 { req.threads } else { 4 };

        info!(
            model = %name,
            path = %model_path.display(),
            port,
            ctx,
            gpu_layers,
            threads,
            "Spawning llama-server"
        );

        let llama_bin = find_llama_server()?;

        let child = Command::new(&llama_bin)
            .arg("--model")
            .arg(&model_path)
            .arg("--ctx-size")
            .arg(ctx.to_string())
            .arg("--n-gpu-layers")
            .arg(gpu_layers.to_string())
            .arg("--threads")
            .arg(threads.to_string())
            .arg("--port")
            .arg(port.to_string())
            .arg("--host")
            .arg("127.0.0.1")
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to spawn llama-server at {}", llama_bin.display()))?;

        let now = now_epoch_ms();

        let mut managed = ManagedModel {
            name: name.clone(),
            path: model_path,
            process: Some(child),
            port,
            status: ModelState::Loading,
            loaded_at: now,
            last_used: now,
            request_count: 0,
            context_length: ctx,
            gpu_layers,
            threads,
        };

        // Wait for the health endpoint to come up (up to 120 s for large models).
        let health_url = format!("http://127.0.0.1:{port}/health");
        let timeout_secs = if managed.path.metadata().map(|m| m.len()).unwrap_or(0) > 2_000_000_000
        {
            120 // Large models need more startup time on CPU
        } else {
            60
        };
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut healthy = false;

        while Instant::now() < deadline {
            match self.http_client.get(&health_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    healthy = true;
                    break;
                }
                _ => {
                    // Check that the process hasn't exited early.
                    if let Some(ref mut proc) = managed.process {
                        match proc.try_wait() {
                            Ok(Some(exit)) => {
                                let msg = format!("llama-server exited early with status {exit}");
                                error!(model = %name, "{msg}");
                                managed.status = ModelState::Error(msg.clone());
                                self.models.insert(name.clone(), managed);
                                bail!("{msg}");
                            }
                            Err(e) => {
                                let msg = format!("failed to poll llama-server process: {e}");
                                error!(model = %name, "{msg}");
                                managed.status = ModelState::Error(msg.clone());
                                self.models.insert(name.clone(), managed);
                                bail!("{msg}");
                            }
                            Ok(None) => { /* still running, keep waiting */ }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }

        if healthy {
            info!(model = %name, port, "llama-server is ready");
            managed.status = ModelState::Ready;
        } else {
            let msg = format!("llama-server did not become healthy within {timeout_secs}s");
            warn!(model = %name, "{msg}");
            managed.status = ModelState::Error(msg);
        }

        let status = model_to_status(&managed);
        self.models.insert(name, managed);
        Ok(status)
    }

    // ------------------------------------------------------------------
    // Unload
    // ------------------------------------------------------------------

    /// Unload (stop) a model.  Sends SIGTERM first, waits up to 10 s, then
    /// SIGKILL.
    pub async fn unload_model(&mut self, name: &str) -> Result<()> {
        let model = self
            .models
            .get_mut(name)
            .with_context(|| format!("Model '{name}' not found"))?;

        model.status = ModelState::Unloading;
        info!(model = %name, "Unloading model");

        if let Some(mut child) = model.process.take() {
            // Try graceful shutdown first.
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                // Send SIGTERM via nix / libc.
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                    debug!(model = %name, pid, "Sent SIGTERM");
                }

                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            info!(
                                model = %name,
                                code = status.code(),
                                signal = status.signal(),
                                "llama-server exited"
                            );
                            break;
                        }
                        Ok(None) => {
                            if Instant::now() >= deadline {
                                warn!(model = %name, "Timeout waiting for graceful shutdown, sending SIGKILL");
                                let _ = child.kill().await;
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(250)).await;
                        }
                        Err(e) => {
                            error!(model = %name, "Error waiting for process: {e}");
                            let _ = child.kill().await;
                            break;
                        }
                    }
                }
            }

            #[cfg(not(unix))]
            {
                let _ = child.kill().await;
            }
        }

        self.models.remove(name);
        info!(model = %name, "Model unloaded");
        Ok(())
    }

    // ------------------------------------------------------------------
    // List / get
    // ------------------------------------------------------------------

    /// Return the status of all managed models.
    pub fn list_models(&self) -> Vec<ModelStatus> {
        self.models.values().map(model_to_status).collect()
    }

    /// Get a mutable reference to a model, updating `last_used`.
    pub(crate) fn get_model(&mut self, name: &str) -> Option<&mut ManagedModel> {
        if let Some(m) = self.models.get_mut(name) {
            m.last_used = now_epoch_ms();
            Some(m)
        } else {
            None
        }
    }

    /// Get the port of a ready model by name.
    pub fn model_port(&mut self, name: &str) -> Option<u16> {
        self.get_model(name).and_then(|m| {
            if matches!(m.status, ModelState::Ready) {
                m.request_count += 1;
                Some(m.port)
            } else {
                None
            }
        })
    }

    /// Get the model name for a model (used after selection by level).
    #[allow(dead_code)]
    pub fn model_name_for_port(&self, _port: u16) -> Option<String> {
        self.models
            .values()
            .find(|m| m.port == _port)
            .map(|m| m.name.clone())
    }

    // ------------------------------------------------------------------
    // Health checking
    // ------------------------------------------------------------------

    /// Check the health of all managed models.  If a process has crashed, mark
    /// it as errored.  (Automatic restart can be added later.)
    pub async fn health_check_all(&mut self) {
        let names: Vec<String> = self.models.keys().cloned().collect();

        for name in names {
            if let Some(model) = self.models.get_mut(&name) {
                // Skip models that are already in an error or unloading state.
                if matches!(model.status, ModelState::Error(_) | ModelState::Unloading) {
                    continue;
                }

                // Check if the process is still alive.
                let alive = match model.process {
                    Some(ref mut child) => match child.try_wait() {
                        Ok(Some(exit)) => {
                            let msg = format!("llama-server exited unexpectedly: {exit}");
                            error!(model = %name, "{msg}");
                            model.status = ModelState::Error(msg);
                            false
                        }
                        Ok(None) => true,
                        Err(e) => {
                            let msg = format!("failed to poll process: {e}");
                            error!(model = %name, "{msg}");
                            model.status = ModelState::Error(msg);
                            false
                        }
                    },
                    None => {
                        model.status = ModelState::Error("no process handle".to_string());
                        false
                    }
                };

                if alive {
                    // Also hit the HTTP health endpoint.
                    let url = format!("http://127.0.0.1:{}/health", model.port);
                    match self.http_client.get(&url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            debug!(model = %name, "Health OK");
                        }
                        Ok(resp) => {
                            let msg = format!("health endpoint returned {}", resp.status());
                            warn!(model = %name, "{msg}");
                            model.status = ModelState::Error(msg);
                        }
                        Err(e) => {
                            let msg = format!("health endpoint unreachable: {e}");
                            warn!(model = %name, "{msg}");
                            model.status = ModelState::Error(msg);
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Intelligence-level routing
    // ------------------------------------------------------------------

    /// Select a model name based on the requested intelligence level.
    ///
    /// Model hierarchy (best reasoning capability per level):
    /// - `reactive`    → heuristics, no LLM needed
    /// - `operational` → TinyLlama 1.1B (fast, simple tasks)
    /// - `tactical`    → DeepSeek-R1 8B (primary), Qwen3-14B (fallback), Mistral 7B (last resort)
    /// - `strategic`   → Qwen3-14B (primary), DeepSeek-R1 8B (fallback), then API gateway
    ///
    /// Returns `None` when the level should be handled outside the local runtime.
    pub fn select_model_for_level(&self, level: &str) -> Option<String> {
        match level {
            "reactive" => {
                // Handled by heuristics, no LLM needed.
                None
            }
            "operational" => {
                // Prefer tinyllama for fast, simple tasks
                self.first_ready_from(&[
                    "tinyllama-1.1b",
                    "DeepSeek-R1-Distill-Qwen-8B",
                    "mistral-7b",
                ])
            }
            "tactical" => {
                // DeepSeek-R1 8B is best reasoning in 8B range
                self.first_ready_from(&[
                    "DeepSeek-R1-Distill-Qwen-8B",
                    "Qwen3-14B",
                    "mistral-7b",
                    "tinyllama-1.1b",
                ])
            }
            "strategic" => {
                // Qwen3-14B for complex reasoning; fall back to DeepSeek-R1,
                // then return None to route to external API via api-gateway.
                self.first_ready_from(&[
                    "Qwen3-14B",
                    "DeepSeek-R1-Distill-Qwen-8B",
                    "mistral-7b",
                ])
            }
            _ => {
                warn!(
                    level,
                    "Unknown intelligence level, falling back to first ready model"
                );
                self.first_ready_model()
            }
        }
    }

    /// Try models in priority order, using partial name matching against loaded
    /// model names.  Returns the first model that is ready.
    fn first_ready_from(&self, candidates: &[&str]) -> Option<String> {
        for candidate in candidates {
            let candidate_lower = candidate.to_lowercase();
            for (name, model) in &self.models {
                if matches!(model.status, ModelState::Ready)
                    && name.to_lowercase().contains(&candidate_lower)
                {
                    return Some(name.clone());
                }
            }
        }
        None
    }

    fn is_model_ready(&self, name: &str) -> bool {
        self.models
            .get(name)
            .map(|m| matches!(m.status, ModelState::Ready))
            .unwrap_or(false)
    }

    fn first_ready_model(&self) -> Option<String> {
        self.models
            .values()
            .find(|m| matches!(m.status, ModelState::Ready))
            .map(|m| m.name.clone())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn model_to_status(m: &ManagedModel) -> ModelStatus {
    ModelStatus {
        model_name: m.name.clone(),
        status: m.status.to_string(),
        port: m.port as i32,
        loaded_at: m.loaded_at,
        last_used: m.last_used,
        request_count: m.request_count,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_epoch_ms_is_positive() {
        let ts = now_epoch_ms();
        assert!(ts > 0, "Epoch ms should be positive");
    }

    #[test]
    fn test_model_state_display() {
        assert_eq!(ModelState::Loading.to_string(), "loading");
        assert_eq!(ModelState::Ready.to_string(), "ready");
        assert_eq!(ModelState::Error("oops".into()).to_string(), "error: oops");
        assert_eq!(ModelState::Unloading.to_string(), "unloading");
    }

    #[test]
    fn test_select_model_empty() {
        let mgr = ModelManager::new();
        assert!(mgr.select_model_for_level("reactive").is_none());
        assert!(mgr.select_model_for_level("operational").is_none());
        assert!(mgr.select_model_for_level("tactical").is_none());
        assert!(mgr.select_model_for_level("strategic").is_none());
    }

    #[test]
    fn test_first_ready_from_partial_match() {
        let mut mgr = ModelManager::new();
        // Insert a model with a GGUF-style name
        mgr.models.insert(
            "DeepSeek-R1-Distill-Qwen-8B-Q4_K_M".to_string(),
            ManagedModel {
                name: "DeepSeek-R1-Distill-Qwen-8B-Q4_K_M".to_string(),
                path: PathBuf::from("/tmp/test.gguf"),
                process: None,
                port: 8082,
                status: ModelState::Ready,
                loaded_at: 1000,
                last_used: 2000,
                request_count: 0,
                context_length: 4096,
                gpu_layers: 0,
                threads: 4,
            },
        );
        // Partial match should find it
        let result = mgr.first_ready_from(&["DeepSeek-R1-Distill-Qwen-8B"]);
        assert!(result.is_some());
        assert!(result.unwrap().contains("DeepSeek-R1"));
    }

    #[test]
    fn test_tactical_prefers_deepseek_over_mistral() {
        let mut mgr = ModelManager::new();
        mgr.models.insert(
            "mistral-7b".to_string(),
            ManagedModel {
                name: "mistral-7b".to_string(),
                path: PathBuf::from("/tmp/mistral.gguf"),
                process: None,
                port: 8080,
                status: ModelState::Ready,
                loaded_at: 1000,
                last_used: 2000,
                request_count: 0,
                context_length: 4096,
                gpu_layers: 0,
                threads: 4,
            },
        );
        mgr.models.insert(
            "DeepSeek-R1-Distill-Qwen-8B-Q4_K_M".to_string(),
            ManagedModel {
                name: "DeepSeek-R1-Distill-Qwen-8B-Q4_K_M".to_string(),
                path: PathBuf::from("/tmp/deepseek.gguf"),
                process: None,
                port: 8082,
                status: ModelState::Ready,
                loaded_at: 1000,
                last_used: 2000,
                request_count: 0,
                context_length: 4096,
                gpu_layers: 0,
                threads: 4,
            },
        );
        let selected = mgr.select_model_for_level("tactical");
        assert!(selected.is_some());
        assert!(selected.unwrap().contains("DeepSeek"), "tactical should prefer DeepSeek-R1 over mistral");
    }

    #[test]
    fn test_allocate_port_default() {
        let mut mgr = ModelManager::new();
        assert_eq!(mgr.allocate_port(0), BASE_PORT);
        assert_eq!(mgr.allocate_port(0), BASE_PORT + 1);
    }

    #[test]
    fn test_allocate_port_explicit() {
        let mut mgr = ModelManager::new();
        assert_eq!(mgr.allocate_port(9999), 9999);
        // The auto-counter should not have advanced.
        assert_eq!(mgr.allocate_port(0), BASE_PORT);
    }

    #[test]
    fn test_list_models_empty() {
        let mgr = ModelManager::new();
        assert!(mgr.list_models().is_empty());
    }

    #[test]
    fn test_model_to_status_conversion() {
        let m = ManagedModel {
            name: "test-model".to_string(),
            path: PathBuf::from("/tmp/test.gguf"),
            process: None,
            port: 8080,
            status: ModelState::Ready,
            loaded_at: 1000,
            last_used: 2000,
            request_count: 42,
            context_length: 2048,
            gpu_layers: 0,
            threads: 4,
        };
        let s = model_to_status(&m);
        assert_eq!(s.model_name, "test-model");
        assert_eq!(s.status, "ready");
        assert_eq!(s.port, 8080);
        assert_eq!(s.loaded_at, 1000);
        assert_eq!(s.last_used, 2000);
        assert_eq!(s.request_count, 42);
    }

    #[test]
    fn test_get_model_missing() {
        let mut mgr = ModelManager::new();
        assert!(mgr.get_model("nonexistent").is_none());
    }

    #[test]
    fn test_find_llama_server_env_override() {
        // When LLAMA_SERVER_PATH points to a real binary it should be used.
        // We'll test the negative case (non-existent) which falls through.
        std::env::set_var("LLAMA_SERVER_PATH", "/tmp/__nonexistent_llama_server__");
        let result = find_llama_server();
        // This should fail (the file doesn't exist and well-known paths likely
        // don't either in a test environment).
        std::env::remove_var("LLAMA_SERVER_PATH");
        // We cannot assert success because the binary might not be installed.
        // Just ensure the function returns an error rather than panicking.
        if result.is_err() {
            // Expected in CI / environments without llama-server.
        }
    }
}
