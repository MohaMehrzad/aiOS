#!/usr/bin/env bash
# ============================================================
# build-llamacpp.sh — Clone and build llama.cpp (llama-server)
# ============================================================
# Produces:
#   build/output/bin/llama-server
#
# Options:
#   --gpu         Build with CUDA support (requires CUDA toolkit)
#   --clean       Force a clean rebuild
#
# Idempotent: skips clone if directory exists; cmake handles
# incremental builds automatically.
# ============================================================
set -euo pipefail

# -----------------------------------------------------------
# Resolve project root
# -----------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# -----------------------------------------------------------
# Parse arguments
# -----------------------------------------------------------
GPU_MODE="off"
CLEAN_BUILD=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --gpu)
            GPU_MODE="on"
            shift
            ;;
        --clean)
            CLEAN_BUILD=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--gpu] [--clean]"
            echo "  --gpu     Build with NVIDIA CUDA support"
            echo "  --clean   Force a clean rebuild (removes build directory)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# -----------------------------------------------------------
# Constants
# -----------------------------------------------------------
LLAMACPP_DIR="build/deps/llama.cpp"
LLAMACPP_BUILD="${LLAMACPP_DIR}/build"
OUTPUT_DIR="build/output"

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[llama.cpp]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[llama.cpp]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[llama.cpp]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[llama.cpp]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Preflight
# -----------------------------------------------------------
for tool in git cmake make; do
    command -v "$tool" >/dev/null 2>&1 || die "Required tool not found: $tool"
done

if [ "$GPU_MODE" = "on" ]; then
    if ! command -v nvcc >/dev/null 2>&1; then
        die "CUDA toolkit not found (nvcc missing). Install CUDA or build without --gpu."
    fi
    info "CUDA detected: $(nvcc --version | grep release | awk '{print $6}')"
fi

# -----------------------------------------------------------
# Step 1 — Clone llama.cpp
# -----------------------------------------------------------
mkdir -p build/deps

if [ ! -d "$LLAMACPP_DIR" ]; then
    info "Cloning llama.cpp repository..."
    git clone --depth 1 https://github.com/ggerganov/llama.cpp.git "$LLAMACPP_DIR"
else
    info "llama.cpp repository already present."
    # Pull latest changes
    info "Updating to latest commit..."
    (cd "$LLAMACPP_DIR" && git pull --ff-only 2>/dev/null) || warn "Could not update (offline or dirty tree)."
fi

# -----------------------------------------------------------
# Step 2 — Clean build if requested
# -----------------------------------------------------------
if [ "$CLEAN_BUILD" = true ] && [ -d "$LLAMACPP_BUILD" ]; then
    info "Cleaning previous build..."
    rm -rf "$LLAMACPP_BUILD"
fi

# -----------------------------------------------------------
# Step 3 — Configure with cmake
# -----------------------------------------------------------
mkdir -p "$LLAMACPP_BUILD"

info "Configuring cmake (GPU=${GPU_MODE})..."

CMAKE_ARGS=(
    -DCMAKE_BUILD_TYPE=Release
    -DLLAMA_BUILD_SERVER=ON
    -DLLAMA_BUILD_EXAMPLES=OFF
    -DLLAMA_BUILD_TESTS=OFF
)

if [ "$GPU_MODE" = "on" ]; then
    CMAKE_ARGS+=(
        -DGGML_CUDA=ON
    )
    info "Building WITH CUDA GPU acceleration."
else
    CMAKE_ARGS+=(
        -DGGML_STATIC=ON
    )
    info "Building CPU-only (static)."
fi

cmake -S "$LLAMACPP_DIR" -B "$LLAMACPP_BUILD" "${CMAKE_ARGS[@]}"

# -----------------------------------------------------------
# Step 4 — Build
# -----------------------------------------------------------
NPROC="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"
info "Building llama-server with ${NPROC} parallel jobs..."

cmake --build "$LLAMACPP_BUILD" --target llama-server -j "$NPROC"

# -----------------------------------------------------------
# Step 5 — Locate and copy the binary
# -----------------------------------------------------------
# The binary location varies by cmake version / llama.cpp version
LLAMA_SERVER_BIN=""
for candidate in \
    "${LLAMACPP_BUILD}/bin/llama-server" \
    "${LLAMACPP_BUILD}/llama-server" \
    "${LLAMACPP_BUILD}/examples/server/llama-server"; do
    if [ -f "$candidate" ]; then
        LLAMA_SERVER_BIN="$candidate"
        break
    fi
done

if [ -z "$LLAMA_SERVER_BIN" ]; then
    die "Build succeeded but llama-server binary not found in expected locations."
fi

mkdir -p "${OUTPUT_DIR}/bin"
cp "$LLAMA_SERVER_BIN" "${OUTPUT_DIR}/bin/llama-server"
chmod 755 "${OUTPUT_DIR}/bin/llama-server"

# -----------------------------------------------------------
# Summary
# -----------------------------------------------------------
BIN_SIZE="$(du -h "${OUTPUT_DIR}/bin/llama-server" | cut -f1)"

echo ""
ok "============================================"
ok " llama.cpp build complete"
ok "============================================"
ok " Binary:  ${OUTPUT_DIR}/bin/llama-server  (${BIN_SIZE})"
ok " GPU:     ${GPU_MODE}"
ok "============================================"
