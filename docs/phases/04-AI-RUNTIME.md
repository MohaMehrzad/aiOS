# Phase 4: Local AI Runtime

## Goal
Get llama.cpp running as a system service inside aiOS, with model management, a gRPC inference API, and the ability to load/unload models based on demand.

## Prerequisites
- Phase 3 complete (system boots with aios-init)
- Read [architecture/SYSTEM.md](../architecture/SYSTEM.md) — AI Runtime section
- Read [VISION.md](../VISION.md) — Intelligence hierarchy

---

## Step-by-Step

### Step 4.1: Cross-Compile llama.cpp

**Claude Code prompt**: "Set up cross-compilation of llama.cpp for x86_64 Linux with CPU inference support, outputting a static server binary"

```bash
# Clone llama.cpp
git clone https://github.com/ggerganov/llama.cpp.git build/deps/llama.cpp
cd build/deps/llama.cpp

# Build the server binary (static, CPU-only for now)
mkdir build && cd build
cmake .. \
    -DCMAKE_BUILD_TYPE=Release \
    -DGGML_STATIC=ON \
    -DLLAMA_BUILD_SERVER=ON \
    -DLLAMA_BUILD_EXAMPLES=OFF \
    -DLLAMA_BUILD_TESTS=OFF
make -j$(nproc) llama-server

# Output: build/bin/llama-server (static binary, ~10-20MB)
```

For GPU support (CUDA):
```bash
cmake .. \
    -DCMAKE_BUILD_TYPE=Release \
    -DLLAMA_BUILD_SERVER=ON \
    -DGGML_CUDA=ON
make -j$(nproc) llama-server
```

### Step 4.1b: Create llama.cpp Build Script

**Claude Code prompt**: "Create the build/build-llamacpp.sh script to clone and compile llama-server"

```bash
#!/bin/bash
# build/build-llamacpp.sh
set -euo pipefail

LLAMACPP_DIR="build/deps/llama.cpp"
OUTPUT_DIR="build/output"
GPU=${1:-"cpu"}  # Pass "cuda" for NVIDIA GPU support

# Clone if not present
if [ ! -d "$LLAMACPP_DIR" ]; then
    echo "Cloning llama.cpp..."
    git clone --depth 1 https://github.com/ggerganov/llama.cpp.git "$LLAMACPP_DIR"
fi

cd "$LLAMACPP_DIR"
mkdir -p build && cd build

# Configure based on GPU support
if [ "$GPU" = "cuda" ]; then
    echo "Building with CUDA support..."
    cmake .. \
        -DCMAKE_BUILD_TYPE=Release \
        -DLLAMA_BUILD_SERVER=ON \
        -DLLAMA_BUILD_EXAMPLES=OFF \
        -DLLAMA_BUILD_TESTS=OFF \
        -DGGML_CUDA=ON
else
    echo "Building CPU-only..."
    cmake .. \
        -DCMAKE_BUILD_TYPE=Release \
        -DGGML_STATIC=ON \
        -DLLAMA_BUILD_SERVER=ON \
        -DLLAMA_BUILD_EXAMPLES=OFF \
        -DLLAMA_BUILD_TESTS=OFF
fi

make -j$(nproc) llama-server

# Copy to output
mkdir -p "../../../$OUTPUT_DIR/bin"
cp bin/llama-server "../../../$OUTPUT_DIR/bin/"

echo "llama-server built successfully:"
ls -lh "../../../$OUTPUT_DIR/bin/llama-server"
```

### Step 4.2: Download Models

**Claude Code prompt**: "Create a model download script that fetches the required GGUF models for all intelligence layers"

```bash
#!/bin/bash
# build/download-models.sh
set -euo pipefail

MODEL_DIR="build/models"
mkdir -p "$MODEL_DIR"

# Operational layer: TinyLlama 1.1B (always loaded, fast, ~700MB)
echo "Downloading TinyLlama 1.1B..."
wget -c -O "$MODEL_DIR/tinyllama-1.1b-chat.Q4_K_M.gguf" \
    "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"

# Tactical layer: Phi-3 Mini 3.8B (loaded on demand, ~2.3GB)
echo "Downloading Phi-3 Mini 3.8B..."
wget -c -O "$MODEL_DIR/phi-3-mini-3.8b.Q4_K_M.gguf" \
    "https://huggingface.co/microsoft/Phi-3-mini-4k-instruct-gguf/resolve/main/Phi-3-mini-4k-instruct-q4.gguf"

# Tactical layer: Mistral 7B (loaded on demand, ~4.4GB)
echo "Downloading Mistral 7B..."
wget -c -O "$MODEL_DIR/mistral-7b-instruct.Q4_K_M.gguf" \
    "https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.2-GGUF/resolve/main/mistral-7b-instruct-v0.2.Q4_K_M.gguf"

echo "All models downloaded to $MODEL_DIR"
ls -lh "$MODEL_DIR"
```

### Step 4.3: Implement `aios-runtime` Daemon

**Claude Code prompt**: "Implement the aios-runtime Rust daemon that manages llama.cpp server instances, handles model loading/unloading, and provides a gRPC inference API"

```
File: agent-core/src/runtime.rs (or separate crate)

Architecture:
  aios-runtime
  ├── ModelManager
  │   ├── load_model(name, config) → starts llama-server instance on a port
  │   ├── unload_model(name) → kills llama-server instance
  │   ├── list_models() → returns loaded models and their ports
  │   └── health_check(name) → pings model endpoint
  ├── InferenceRouter
  │   ├── infer(request) → routes to correct model based on layer/capability
  │   ├── queue management → prevents overloading a single model
  │   └── timeout handling
  └── gRPC Service
      ├── LoadModel(request) → ModelStatus
      ├── UnloadModel(request) → Status
      ├── Infer(InferRequest) → InferResponse
      ├── StreamInfer(InferRequest) → stream InferChunk
      └── ListModels(Empty) → ModelList
```

### gRPC Proto Definition

```protobuf
// agent-core/proto/runtime.proto
syntax = "proto3";
package aios.runtime;

service AIRuntime {
    rpc LoadModel(LoadModelRequest) returns (ModelStatus);
    rpc UnloadModel(UnloadModelRequest) returns (Status);
    rpc ListModels(Empty) returns (ModelList);
    rpc Infer(InferRequest) returns (InferResponse);
    rpc StreamInfer(InferRequest) returns (stream InferChunk);
    rpc HealthCheck(Empty) returns (HealthStatus);
}

message LoadModelRequest {
    string model_name = 1;       // e.g., "tinyllama-1.1b"
    string model_path = 2;       // e.g., "/var/lib/aios/models/tinyllama..."
    int32 context_length = 3;    // e.g., 2048
    int32 gpu_layers = 4;        // 0 = CPU only, -1 = all on GPU
    int32 threads = 5;           // CPU threads to use
    int32 port = 6;              // Port for llama-server (0 = auto-assign)
}

message InferRequest {
    string model = 1;            // Which model to use (or "auto" for routing)
    string prompt = 2;
    string system_prompt = 3;
    int32 max_tokens = 4;
    float temperature = 5;
    string intelligence_level = 6;  // "operational", "tactical", "strategic"
}

message InferResponse {
    string text = 1;
    int32 tokens_used = 2;
    int64 latency_ms = 3;
    string model_used = 4;
}

message InferChunk {
    string text = 1;
    bool done = 2;
}
```

### Step 4.4: Model Manager Implementation

**Claude Code prompt**: "Implement the ModelManager that spawns llama-server processes, manages their lifecycle, and handles health checks"

Key behaviors:
1. On startup: load the operational model (TinyLlama) — MUST succeed or init fails
2. On demand: load tactical models when requested by orchestrator
3. Auto-unload: unload tactical models after idle timeout (configurable, default 5 min)
4. Health check: ping each loaded model every 10 seconds
5. Crash recovery: if a llama-server process dies, restart it automatically

```rust
// Pseudocode for ModelManager
struct ModelManager {
    models: HashMap<String, LoadedModel>,
    config: RuntimeConfig,
}

struct LoadedModel {
    name: String,
    process: Child,          // llama-server process
    port: u16,
    last_used: Instant,
    status: ModelStatus,
}

impl ModelManager {
    async fn load_model(&mut self, req: LoadModelRequest) -> Result<ModelStatus> {
        // 1. Verify model file exists
        // 2. Find free port
        // 3. Spawn llama-server process:
        //    llama-server -m <model_path> --port <port> -c <context_length>
        //                 -ngl <gpu_layers> -t <threads> --log-disable
        // 4. Wait for health check to pass (retry for up to 30s)
        // 5. Register in models map
        // 6. Return status
    }

    async fn unload_model(&mut self, name: &str) -> Result<()> {
        // 1. Send SIGTERM to llama-server process
        // 2. Wait for exit (timeout 5s, then SIGKILL)
        // 3. Remove from models map
    }

    async fn health_check_loop(&self) {
        // Every 10 seconds:
        // - Ping each model's /health endpoint
        // - If model doesn't respond: restart it
        // - Check idle timeout: unload if not used recently
    }
}
```

### Step 4.5: Inference Router

**Claude Code prompt**: "Implement the InferenceRouter that maps intelligence levels to loaded models and handles the inference request lifecycle"

```python
# Routing logic (in both Rust service and Python client)

INTELLIGENCE_ROUTING = {
    "operational": ["tinyllama-1.1b"],        # Tiny, always loaded
    "tactical": ["mistral-7b", "phi-3-3.8b"], # Medium, load on demand
    "strategic": None,                         # Handled by API Gateway, not local
}

async def route_inference(request: InferRequest) -> InferResponse:
    level = request.intelligence_level or classify_request(request)

    if level == "strategic":
        # Forward to API Gateway (Claude/OpenAI)
        return await api_gateway.infer(request)

    models = INTELLIGENCE_ROUTING[level]
    for model_name in models:
        if model_manager.is_loaded(model_name):
            return await model_manager.infer(model_name, request)

    # Model not loaded — load it
    await model_manager.load_model(models[0])
    return await model_manager.infer(models[0], request)
```

### Step 4.6: Integrate with aios-init

**Claude Code prompt**: "Update aios-init to start the aios-runtime service and verify the operational model loads successfully"

Update the init boot sequence:
```
Phase 2: AI RUNTIME
  1. Start aios-runtime daemon
  2. aios-runtime loads TinyLlama 1.1B
  3. aios-init sends test prompt: "Respond with OK if you are working."
  4. If response contains "OK" → Phase 2 complete
  5. If failure → log error, retry 3 times, then panic
```

### Step 4.7: Test Inference

**Claude Code prompt**: "Create a test script that boots aiOS in QEMU and verifies local model inference works end-to-end"

```bash
# Test: send a prompt to the runtime and get a response
# From inside the booted VM:
grpcurl -plaintext localhost:50051 aios.runtime.AIRuntime/Infer \
    -d '{"prompt": "What is 2+2?", "max_tokens": 50, "intelligence_level": "operational"}'

# Expected: {"text": "2+2 is 4.", "tokens_used": 8, "model_used": "tinyllama-1.1b"}
```

---

## Deliverables Checklist

- [ ] llama.cpp compiled (static binary)
- [ ] At least TinyLlama 1.1B model downloaded
- [ ] `aios-runtime` daemon starts and manages llama-server processes
- [ ] Operational model loads automatically at boot
- [ ] gRPC inference API works (send prompt → get response)
- [ ] Model health checking works
- [ ] Idle model unloading works
- [ ] aios-init starts aios-runtime and verifies operational model
- [ ] End-to-end test: boot → load model → send prompt → get response

---

## Performance Targets

| Metric | Target | Notes |
|---|---|---|
| Model load time (TinyLlama) | <5 seconds | First boot |
| Inference latency (TinyLlama) | <200ms | 50 token response |
| Inference latency (Mistral 7B) | <500ms | 50 token response, CPU |
| Inference latency (Mistral 7B) | <100ms | 50 token response, GPU |
| Memory usage (TinyLlama loaded) | <1 GB | Q4 quantization |
| Memory usage (Mistral 7B loaded) | <5 GB | Q4 quantization |

---

## Next Phase
Once inference test passes → [Phase 5: Agent Core](./05-AGENT-CORE.md)
