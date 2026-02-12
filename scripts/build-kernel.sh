#!/usr/bin/env bash
# ============================================================
# build-kernel.sh — Download, configure, and build the aiOS kernel
# ============================================================
# Produces:
#   build/output/vmlinuz          — compressed kernel image
#   build/output/modules/         — installed kernel modules
#
# Idempotent: re-running skips download/extract if already present
#             and only rebuilds changed objects.
# ============================================================
set -euo pipefail

# -----------------------------------------------------------
# Resolve project root (one level above this script)
# -----------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# -----------------------------------------------------------
# Constants
# -----------------------------------------------------------
KERNEL_VERSION="6.8.12"
KERNEL_MAJOR="6"
KERNEL_TARBALL="linux-${KERNEL_VERSION}.tar.xz"
KERNEL_URL="https://cdn.kernel.org/pub/linux/kernel/v${KERNEL_MAJOR}.x/${KERNEL_TARBALL}"
KERNEL_SRC_DIR="kernel/src/linux-${KERNEL_VERSION}"
OVERLAY_CONFIG="kernel/configs/aios-kernel.config"
OUTPUT_DIR="build/output"

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[kernel]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[kernel]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[kernel]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[kernel]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Preflight checks
# -----------------------------------------------------------
for tool in make gcc flex bison bc perl wget xz; do
    command -v "$tool" >/dev/null 2>&1 || die "Required tool not found: $tool"
done

if [ ! -f "$OVERLAY_CONFIG" ]; then
    die "Kernel overlay config not found at $OVERLAY_CONFIG"
fi

# -----------------------------------------------------------
# Step 1 — Download kernel source
# -----------------------------------------------------------
mkdir -p kernel/src

if [ ! -f "kernel/src/${KERNEL_TARBALL}" ]; then
    info "Downloading Linux ${KERNEL_VERSION} source..."
    wget -q --show-progress -O "kernel/src/${KERNEL_TARBALL}" "$KERNEL_URL"
else
    info "Kernel tarball already present, skipping download."
fi

# -----------------------------------------------------------
# Step 2 — Extract
# -----------------------------------------------------------
if [ ! -d "$KERNEL_SRC_DIR" ]; then
    info "Extracting kernel source..."
    tar xf "kernel/src/${KERNEL_TARBALL}" -C kernel/src/
else
    info "Kernel source directory already exists, skipping extraction."
fi

# -----------------------------------------------------------
# Step 3 — Configure: tinyconfig + overlay merge
# -----------------------------------------------------------
info "Generating kernel configuration (tinyconfig + aiOS overlay)..."
cd "$KERNEL_SRC_DIR"

# Start from the smallest possible base
make tinyconfig

# Merge our overlay on top.  merge_config.sh is part of the
# kernel source tree and handles symbol dependencies.
KCONFIG_CONFIG=.config scripts/kconfig/merge_config.sh \
    .config \
    "${PROJECT_ROOT}/${OVERLAY_CONFIG}"

# Resolve any unset dependencies introduced by the merge
make olddefconfig

cd "$PROJECT_ROOT"

# -----------------------------------------------------------
# Step 4 — Build
# -----------------------------------------------------------
NPROC="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"
info "Building kernel with ${NPROC} parallel jobs..."
make -C "$KERNEL_SRC_DIR" -j"$NPROC"

# Build modules
info "Building kernel modules..."
make -C "$KERNEL_SRC_DIR" -j"$NPROC" modules

# -----------------------------------------------------------
# Step 5 — Install artifacts to build/output
# -----------------------------------------------------------
mkdir -p "$OUTPUT_DIR"

info "Copying bzImage..."
cp "${KERNEL_SRC_DIR}/arch/x86/boot/bzImage" "${OUTPUT_DIR}/vmlinuz"

info "Installing modules..."
rm -rf "${OUTPUT_DIR}/modules"
make -C "$KERNEL_SRC_DIR" modules_install \
    INSTALL_MOD_PATH="${PROJECT_ROOT}/${OUTPUT_DIR}/modules"

# -----------------------------------------------------------
# Step 6 — Summary
# -----------------------------------------------------------
VMLINUZ_SIZE="$(du -h "${OUTPUT_DIR}/vmlinuz" | cut -f1)"
MODULE_COUNT="$(find "${OUTPUT_DIR}/modules" -name '*.ko' -o -name '*.ko.zst' -o -name '*.ko.xz' 2>/dev/null | wc -l | tr -d ' ')"
MODULES_SIZE="$(du -sh "${OUTPUT_DIR}/modules" 2>/dev/null | cut -f1)"

echo ""
ok "============================================"
ok " Kernel build complete"
ok "============================================"
ok " Image:     ${OUTPUT_DIR}/vmlinuz  (${VMLINUZ_SIZE})"
ok " Modules:   ${OUTPUT_DIR}/modules/ (${MODULE_COUNT} modules, ${MODULES_SIZE})"
ok "============================================"
