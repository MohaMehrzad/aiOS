#!/usr/bin/env bash
# ============================================================
# download-models.sh — Download GGUF model files for aiOS
# ============================================================
# Produces:
#   build/output/models/tinyllama-1.1b-chat.Q4_K_M.gguf   (always)
#   build/output/models/mistral-7b-instruct.Q4_K_M.gguf   (with --tactical)
#
# Options:
#   --tactical    Also download Mistral 7B (larger, ~4.4 GB)
#   --all         Download all available models
#   --verify      Only verify existing downloads, don't download
#
# Idempotent: resumes partial downloads with wget -c.
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
DOWNLOAD_TACTICAL=false
VERIFY_ONLY=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tactical)
            DOWNLOAD_TACTICAL=true
            shift
            ;;
        --all)
            DOWNLOAD_TACTICAL=true
            shift
            ;;
        --verify)
            VERIFY_ONLY=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--tactical] [--all] [--verify]"
            echo ""
            echo "Downloads GGUF model files for aiOS local inference."
            echo ""
            echo "Options:"
            echo "  --tactical   Also download Mistral 7B Q4_K_M (~4.4 GB)"
            echo "  --all        Download all supported models"
            echo "  --verify     Verify existing downloads without downloading"
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
MODEL_DIR="build/output/models"

# Model definitions: name | URL | expected minimum size in bytes
# TinyLlama 1.1B Q4_K_M — always downloaded (operational layer)
TINYLLAMA_NAME="tinyllama-1.1b-chat.Q4_K_M.gguf"
TINYLLAMA_URL="https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
TINYLLAMA_MIN_SIZE=600000000    # ~669 MB, sanity floor at 600 MB

# Mistral 7B Instruct Q4_K_M — tactical layer (optional)
MISTRAL_NAME="mistral-7b-instruct.Q4_K_M.gguf"
MISTRAL_URL="https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.2-GGUF/resolve/main/mistral-7b-instruct-v0.2.Q4_K_M.gguf"
MISTRAL_MIN_SIZE=4000000000     # ~4.37 GB, sanity floor at 4 GB

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[models]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[models]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[models]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[models]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Helper: download and verify a single model
# -----------------------------------------------------------
download_model() {
    local name="$1"
    local url="$2"
    local min_size="$3"
    local filepath="${MODEL_DIR}/${name}"

    if [ "$VERIFY_ONLY" = true ]; then
        verify_model "$name" "$min_size"
        return
    fi

    if [ -f "$filepath" ]; then
        local current_size
        current_size="$(stat -f%z "$filepath" 2>/dev/null || stat -c%s "$filepath" 2>/dev/null || echo 0)"
        if [ "$current_size" -ge "$min_size" ]; then
            info "${name} already downloaded and verified ($(format_size "$current_size"))."
            return 0
        else
            warn "${name} exists but is too small ($(format_size "$current_size")), re-downloading..."
        fi
    fi

    info "Downloading ${name}..."
    info "  URL: ${url}"
    info "  Expected size: >= $(format_size "$min_size")"
    echo ""

    wget -c --show-progress --progress=bar:force:noscroll \
         -O "$filepath" "$url" 2>&1

    echo ""
    verify_model "$name" "$min_size"
}

# -----------------------------------------------------------
# Helper: verify a downloaded model
# -----------------------------------------------------------
verify_model() {
    local name="$1"
    local min_size="$2"
    local filepath="${MODEL_DIR}/${name}"

    if [ ! -f "$filepath" ]; then
        warn "MISSING: ${name} not found at ${filepath}"
        return 1
    fi

    local actual_size
    actual_size="$(stat -f%z "$filepath" 2>/dev/null || stat -c%s "$filepath" 2>/dev/null || echo 0)"

    if [ "$actual_size" -lt "$min_size" ]; then
        warn "CORRUPT: ${name} is too small ($(format_size "$actual_size"), expected >= $(format_size "$min_size"))"
        warn "  Delete ${filepath} and re-run to redownload."
        return 1
    fi

    ok "VERIFIED: ${name} — $(format_size "$actual_size")"
    return 0
}

# -----------------------------------------------------------
# Helper: format byte count as human-readable
# -----------------------------------------------------------
format_size() {
    local bytes="$1"
    if [ "$bytes" -ge 1073741824 ]; then
        printf "%.1f GB" "$(echo "scale=1; $bytes / 1073741824" | bc 2>/dev/null || echo "?")"
    elif [ "$bytes" -ge 1048576 ]; then
        printf "%.1f MB" "$(echo "scale=1; $bytes / 1048576" | bc 2>/dev/null || echo "?")"
    else
        printf "%d bytes" "$bytes"
    fi
}

# -----------------------------------------------------------
# Preflight
# -----------------------------------------------------------
command -v wget >/dev/null 2>&1 || die "Required tool not found: wget"

mkdir -p "$MODEL_DIR"

# -----------------------------------------------------------
# Download models
# -----------------------------------------------------------
echo ""
info "========================================"
info " aiOS Model Downloader"
info "========================================"
echo ""

TOTAL_MODELS=1
if [ "$DOWNLOAD_TACTICAL" = true ]; then
    TOTAL_MODELS=2
fi

# 1) TinyLlama 1.1B — always required (operational layer)
info "[1/${TOTAL_MODELS}] TinyLlama 1.1B Chat Q4_K_M (operational layer)"
download_model "$TINYLLAMA_NAME" "$TINYLLAMA_URL" "$TINYLLAMA_MIN_SIZE"
echo ""

# 2) Mistral 7B — optional (tactical layer)
if [ "$DOWNLOAD_TACTICAL" = true ]; then
    info "[2/${TOTAL_MODELS}] Mistral 7B Instruct Q4_K_M (tactical layer)"
    download_model "$MISTRAL_NAME" "$MISTRAL_URL" "$MISTRAL_MIN_SIZE"
    echo ""
fi

# -----------------------------------------------------------
# Summary
# -----------------------------------------------------------
echo ""
ok "========================================"
ok " Model download complete"
ok "========================================"
ok " Location: ${MODEL_DIR}/"
echo ""
info "Installed models:"

TOTAL_SIZE=0
for model_file in "${MODEL_DIR}"/*.gguf; do
    [ -f "$model_file" ] || continue
    model_name="$(basename "$model_file")"
    model_size="$(stat -f%z "$model_file" 2>/dev/null || stat -c%s "$model_file" 2>/dev/null || echo 0)"
    TOTAL_SIZE=$((TOTAL_SIZE + model_size))
    info "  ${model_name}  ($(format_size "$model_size"))"
done

if [ "$TOTAL_SIZE" -eq 0 ]; then
    warn "  (no models installed)"
else
    echo ""
    ok "Total: $(format_size "$TOTAL_SIZE")"
fi
