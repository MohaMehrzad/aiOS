#!/usr/bin/env bash
# ============================================================
# build-all.sh — Master build orchestrator for aiOS
# ============================================================
# Builds all components in dependency order:
#   1. Kernel
#   2. Initramfs
#   3. Rust workspace (aios-init, aios-orchestrator, aios-tools,
#      aios-memory, aios-api-gateway)
#   4. llama.cpp (llama-server)
#   5. Download models (optional)
#   6. Root filesystem
#   7. ISO image
#
# Usage:
#   ./scripts/build-all.sh                    # Full build
#   ./scripts/build-all.sh --skip-kernel      # Skip kernel build
#   ./scripts/build-all.sh --skip-models      # Skip model download
#   ./scripts/build-all.sh --gpu              # Build llama.cpp with CUDA
#   ./scripts/build-all.sh --tactical-models  # Also download Mistral 7B
#
# Idempotent: each sub-script handles its own caching.
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
SKIP_KERNEL=false
SKIP_MODELS=false
GPU_MODE=false
TACTICAL_MODELS=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-kernel)
            SKIP_KERNEL=true
            shift
            ;;
        --skip-models)
            SKIP_MODELS=true
            shift
            ;;
        --gpu)
            GPU_MODE=true
            shift
            ;;
        --tactical-models)
            TACTICAL_MODELS=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --skip-kernel       Skip kernel build (use existing)"
            echo "  --skip-models       Skip model download"
            echo "  --gpu               Build llama.cpp with CUDA support"
            echo "  --tactical-models   Also download Mistral 7B model"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[build]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[build]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[build]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[build]\033[0m %s\n' "$*" >&2; exit 1; }
banner() {
    echo ""
    printf '\033[1;36m'
    printf '═══════════════════════════════════════════════════\n'
    printf ' %s\n' "$*"
    printf '═══════════════════════════════════════════════════\n'
    printf '\033[0m'
    echo ""
}

# -----------------------------------------------------------
# Timer helpers
# -----------------------------------------------------------
BUILD_START="$(date +%s)"

step_start() {
    STEP_START="$(date +%s)"
}

step_done() {
    local elapsed=$(( $(date +%s) - STEP_START ))
    ok "$1 (${elapsed}s)"
}

# -----------------------------------------------------------
# Preflight — check for required build tools
# -----------------------------------------------------------
banner "Checking build prerequisites"

REQUIRED_TOOLS=(
    "make:build-essential / make"
    "gcc:build-essential / gcc"
    "git:git"
    "wget:wget"
    "cmake:cmake"
    "cargo:rustup (https://rustup.rs)"
)

OPTIONAL_TOOLS=(
    "qemu-system-x86_64:qemu-system-x86 (for testing)"
    "grub-mkrescue:grub2-common (for ISO building)"
)

MISSING_REQUIRED=0
for entry in "${REQUIRED_TOOLS[@]}"; do
    tool="${entry%%:*}"
    package="${entry##*:}"
    if command -v "$tool" >/dev/null 2>&1; then
        ok "  Found: $tool"
    else
        warn "  MISSING: $tool  (install: $package)"
        MISSING_REQUIRED=$((MISSING_REQUIRED + 1))
    fi
done

echo ""
for entry in "${OPTIONAL_TOOLS[@]}"; do
    tool="${entry%%:*}"
    package="${entry##*:}"
    if command -v "$tool" >/dev/null 2>&1; then
        info "  Found (optional): $tool"
    else
        info "  Not found (optional): $tool — $package"
    fi
done

if [ "$MISSING_REQUIRED" -gt 0 ]; then
    die "${MISSING_REQUIRED} required tool(s) missing. Install them and retry."
fi

# Check for musl target
if ! rustup target list --installed 2>/dev/null | grep -q x86_64-unknown-linux-musl; then
    warn "Rust musl target not installed. Adding it now..."
    rustup target add x86_64-unknown-linux-musl || warn "Could not add musl target (may need manual install)"
fi

echo ""
ok "All required tools present."

# -----------------------------------------------------------
# Create output directory
# -----------------------------------------------------------
mkdir -p build/output

# ===========================================================
# Step 1 — Kernel
# ===========================================================
if [ "$SKIP_KERNEL" = true ]; then
    banner "[1/7] Kernel — SKIPPED"
    if [ ! -f build/output/vmlinuz ]; then
        warn "No pre-built kernel at build/output/vmlinuz. Subsequent steps may fail."
    fi
else
    banner "[1/7] Building Linux Kernel"
    step_start
    bash scripts/build-kernel.sh
    step_done "Kernel build complete"
fi

# ===========================================================
# Step 2 — Initramfs
# ===========================================================
banner "[2/7] Building Initramfs"
step_start
bash scripts/build-initramfs.sh
step_done "Initramfs build complete"

# ===========================================================
# Step 3 — Rust workspace
# ===========================================================
banner "[3/7] Building Rust Workspace"
step_start

info "Building all Rust crates in release mode (musl target)..."
cargo build \
    --workspace \
    --release \
    --target x86_64-unknown-linux-musl

# Copy binaries to output
mkdir -p build/output/bin
for bin_name in aios-init aios-orchestrator aios-tools aios-memory aios-api-gateway; do
    src="target/x86_64-unknown-linux-musl/release/${bin_name}"
    if [ -f "$src" ]; then
        cp "$src" "build/output/bin/${bin_name}"
        ok "  Built: ${bin_name} ($(du -h "$src" | cut -f1))"
    else
        warn "  Binary not produced: ${bin_name}"
    fi
done

step_done "Rust workspace build complete"

# ===========================================================
# Step 4 — llama.cpp
# ===========================================================
banner "[4/7] Building llama.cpp"
step_start

LLAMA_ARGS=()
if [ "$GPU_MODE" = true ]; then
    LLAMA_ARGS+=(--gpu)
fi
bash scripts/build-llamacpp.sh "${LLAMA_ARGS[@]}"

step_done "llama.cpp build complete"

# ===========================================================
# Step 5 — Download models
# ===========================================================
if [ "$SKIP_MODELS" = true ]; then
    banner "[5/7] Model Download — SKIPPED"
else
    banner "[5/7] Downloading AI Models"
    step_start

    MODEL_ARGS=()
    if [ "$TACTICAL_MODELS" = true ]; then
        MODEL_ARGS+=(--tactical)
    fi
    bash scripts/download-models.sh "${MODEL_ARGS[@]}"

    step_done "Model download complete"
fi

# ===========================================================
# Step 6 — Root filesystem
# ===========================================================
banner "[6/7] Building Root Filesystem"
step_start
bash scripts/build-rootfs.sh
step_done "Root filesystem build complete"

# ===========================================================
# Step 7 — ISO
# ===========================================================
banner "[7/7] Building ISO Image"
step_start

if command -v grub-mkrescue >/dev/null 2>&1 || command -v grub2-mkrescue >/dev/null 2>&1 || command -v xorriso >/dev/null 2>&1; then
    bash scripts/build-iso.sh
    step_done "ISO build complete"
else
    warn "No ISO builder (grub-mkrescue or xorriso) found — skipping ISO creation."
    info "You can still boot directly with: ./scripts/run-qemu.sh"
fi

# ===========================================================
# Build summary
# ===========================================================
BUILD_ELAPSED=$(( $(date +%s) - BUILD_START ))
BUILD_MINUTES=$(( BUILD_ELAPSED / 60 ))
BUILD_SECONDS=$(( BUILD_ELAPSED % 60 ))

banner "Build Complete!"

ok "Total build time: ${BUILD_MINUTES}m ${BUILD_SECONDS}s"
echo ""

info "Build artifacts:"
for artifact in vmlinuz initramfs.img rootfs.img aios.iso; do
    path="build/output/${artifact}"
    if [ -f "$path" ]; then
        ok "  ${path}  ($(du -h "$path" | cut -f1))"
    fi
done

if [ -d "build/output/bin" ]; then
    echo ""
    info "Binaries:"
    for bin_file in build/output/bin/*; do
        [ -f "$bin_file" ] || continue
        ok "  ${bin_file}  ($(du -h "$bin_file" | cut -f1))"
    done
fi

if [ -d "build/output/models" ]; then
    echo ""
    info "Models:"
    for model_file in build/output/models/*.gguf; do
        [ -f "$model_file" ] || continue
        ok "  $(basename "$model_file")  ($(du -h "$model_file" | cut -f1))"
    done
fi

echo ""
ok "============================================"
ok " Next steps:"
ok "   Boot in QEMU:  ./scripts/run-qemu.sh"
ok "   Boot from ISO:  ./scripts/run-qemu.sh --iso"
ok "============================================"
