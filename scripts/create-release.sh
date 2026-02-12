#!/usr/bin/env bash
# ============================================================
# create-release.sh — aiOS Release Packaging
# ============================================================
# Builds all components and packages them into a release archive.
#
# Produces:
#   releases/<version>/
#     aios-<version>.iso        — Bootable ISO image
#     SHA256SUMS                — Checksums for all artifacts
#     RELEASE-NOTES.md          — Release notes (template filled in)
#     aios-<version>-release.tar.gz — Archive of the entire release
#
# Usage:
#   ./scripts/create-release.sh                 # Auto-detect version
#   ./scripts/create-release.sh --version 0.2.0 # Explicit version
#   ./scripts/create-release.sh --skip-build    # Package existing build
#
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
SKIP_BUILD=false
EXPLICIT_VERSION=""
BUILD_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            EXPLICIT_VERSION="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --skip-kernel)
            BUILD_ARGS+=("--skip-kernel")
            shift
            ;;
        --skip-models)
            BUILD_ARGS+=("--skip-models")
            shift
            ;;
        --gpu)
            BUILD_ARGS+=("--gpu")
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --version VER    Set release version explicitly (default: from Cargo.toml)"
            echo "  --skip-build     Skip build step, package existing artifacts"
            echo "  --skip-kernel    Pass --skip-kernel to build-all.sh"
            echo "  --skip-models    Pass --skip-models to build-all.sh"
            echo "  --gpu            Pass --gpu to build-all.sh"
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
info()  { printf '\033[1;34m[release]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[release]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[release]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[release]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Determine version
# -----------------------------------------------------------
if [ -n "$EXPLICIT_VERSION" ]; then
    VERSION="$EXPLICIT_VERSION"
else
    # Extract version from workspace Cargo.toml or initd Cargo.toml
    VERSION=""
    for cargo_file in Cargo.toml initd/Cargo.toml; do
        if [ -f "$cargo_file" ]; then
            VERSION="$(grep -m1 '^version' "$cargo_file" | sed 's/.*"\(.*\)".*/\1/' || true)"
            if [ -n "$VERSION" ]; then
                break
            fi
        fi
    done
    if [ -z "$VERSION" ]; then
        VERSION="0.1.0"
        warn "Could not determine version, defaulting to ${VERSION}"
    fi
fi

# Generate version tag
DATE_TAG="$(date -u '+%Y%m%d')"
VERSION_TAG="v${VERSION}"
RELEASE_NAME="aios-${VERSION}"

info "========================================"
info " aiOS Release Builder"
info " Version: ${VERSION_TAG}"
info " Date:    ${DATE_TAG}"
info "========================================"
echo ""

# -----------------------------------------------------------
# Step 1: Run build-all.sh
# -----------------------------------------------------------
if [ "$SKIP_BUILD" = true ]; then
    info "[1/5] Build — SKIPPED (using existing artifacts)"
else
    info "[1/5] Running full build..."
    echo ""
    bash scripts/build-all.sh "${BUILD_ARGS[@]}"
    echo ""
    ok "Build complete"
fi

# -----------------------------------------------------------
# Step 2: Verify build artifacts exist
# -----------------------------------------------------------
info "[2/5] Verifying build artifacts..."

REQUIRED_ARTIFACTS=()
OPTIONAL_ARTIFACTS=()

# The ISO is the primary deliverable
if [ -f "build/output/aios.iso" ]; then
    REQUIRED_ARTIFACTS+=("build/output/aios.iso")
    ok "  Found: aios.iso"
else
    warn "  Missing: aios.iso"
fi

# Kernel and initramfs
for artifact in build/output/vmlinuz build/output/initramfs.img; do
    if [ -f "$artifact" ]; then
        REQUIRED_ARTIFACTS+=("$artifact")
        ok "  Found: $(basename "$artifact")"
    else
        warn "  Missing: $(basename "$artifact")"
    fi
done

# Root filesystem image
if [ -f "build/output/rootfs.img" ]; then
    REQUIRED_ARTIFACTS+=("build/output/rootfs.img")
    ok "  Found: rootfs.img"
else
    warn "  Missing: rootfs.img"
fi

# Binaries (optional in release, they're in the rootfs)
if [ -d "build/output/bin" ]; then
    for bin_file in build/output/bin/*; do
        [ -f "$bin_file" ] || continue
        OPTIONAL_ARTIFACTS+=("$bin_file")
    done
    ok "  Found: $(ls -1 build/output/bin/ 2>/dev/null | wc -l | tr -d ' ') binaries"
fi

# Models (optional)
if [ -d "build/output/models" ]; then
    model_count="$(ls -1 build/output/models/*.gguf 2>/dev/null | wc -l | tr -d ' ')"
    if [ "$model_count" -gt 0 ]; then
        ok "  Found: ${model_count} model(s)"
    fi
fi

if [ ${#REQUIRED_ARTIFACTS[@]} -eq 0 ]; then
    die "No build artifacts found. Run build-all.sh first or remove --skip-build."
fi

# -----------------------------------------------------------
# Step 3: Create release directory
# -----------------------------------------------------------
info "[3/5] Creating release directory..."

RELEASE_DIR="releases/${VERSION_TAG}"
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"

# Copy ISO
if [ -f "build/output/aios.iso" ]; then
    cp "build/output/aios.iso" "${RELEASE_DIR}/${RELEASE_NAME}.iso"
    ok "  Copied ISO: ${RELEASE_NAME}.iso"
fi

# Copy kernel and initramfs (useful for PXE/netboot)
if [ -f "build/output/vmlinuz" ]; then
    cp "build/output/vmlinuz" "${RELEASE_DIR}/"
    ok "  Copied: vmlinuz"
fi

if [ -f "build/output/initramfs.img" ]; then
    cp "build/output/initramfs.img" "${RELEASE_DIR}/"
    ok "  Copied: initramfs.img"
fi

# Copy rootfs image
if [ -f "build/output/rootfs.img" ]; then
    cp "build/output/rootfs.img" "${RELEASE_DIR}/${RELEASE_NAME}-rootfs.img"
    ok "  Copied: ${RELEASE_NAME}-rootfs.img"
fi

# -----------------------------------------------------------
# Step 4: Generate checksums and release notes
# -----------------------------------------------------------
info "[4/5] Generating checksums and release notes..."

# Generate SHA256 checksums
(
    cd "$RELEASE_DIR"
    sha256sum -- * 2>/dev/null > SHA256SUMS || shasum -a 256 -- * > SHA256SUMS 2>/dev/null || {
        # Fallback: compute manually
        for f in *; do
            [ -f "$f" ] && [ "$f" != "SHA256SUMS" ] || continue
            if command -v sha256sum >/dev/null 2>&1; then
                sha256sum "$f"
            elif command -v shasum >/dev/null 2>&1; then
                shasum -a 256 "$f"
            else
                openssl dgst -sha256 "$f" | sed 's/.*= //' | tr -d '\n'
                printf '  %s\n' "$f"
            fi
        done > SHA256SUMS
    }
)
ok "  Generated SHA256SUMS"

# Collect git information if available
GIT_COMMIT="unknown"
GIT_BRANCH="unknown"
GIT_TAG_INFO=""
if command -v git >/dev/null 2>&1 && [ -d .git ]; then
    GIT_COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo 'unknown')"
    GIT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'unknown')"
    GIT_TAG_INFO="$(git describe --tags --always 2>/dev/null || echo '')"
fi

# Generate release notes
cat > "${RELEASE_DIR}/RELEASE-NOTES.md" << NOTES
# aiOS ${VERSION_TAG} Release Notes

**Release Date:** $(date -u '+%Y-%m-%d')
**Build Date:** $(date -u '+%Y-%m-%d %H:%M:%S UTC')
**Git Commit:** ${GIT_COMMIT}
**Git Branch:** ${GIT_BRANCH}

## Overview

aiOS is an AI-native operating system that runs AI agents as first-class processes.
This release includes the bootable ISO, kernel, initramfs, and root filesystem.

## What's Included

| Artifact | Description |
|----------|-------------|
| ${RELEASE_NAME}.iso | Bootable hybrid ISO (BIOS + EFI) |
| vmlinuz | Linux kernel image |
| initramfs.img | Initial RAM filesystem |
| ${RELEASE_NAME}-rootfs.img | Root filesystem disk image |
| SHA256SUMS | SHA-256 checksums for verification |

## Installation

### Quick Start (QEMU)

\`\`\`bash
qemu-system-x86_64 \\
    -cdrom ${RELEASE_NAME}.iso \\
    -m 4G \\
    -smp 2 \\
    -nographic \\
    -drive file=aios-disk.img,format=qcow2,if=virtio
\`\`\`

### Bare Metal / VM Install

1. Write the ISO to a USB drive:
   \`\`\`bash
   sudo dd if=${RELEASE_NAME}.iso of=/dev/sdX bs=4M status=progress
   \`\`\`
2. Boot from the USB drive
3. Select "Install aiOS" from the GRUB menu
4. Follow the installer prompts

### Verify Download

\`\`\`bash
sha256sum -c SHA256SUMS
\`\`\`

## System Requirements

- **CPU:** x86_64 (64-bit)
- **RAM:** 2 GB minimum, 4 GB recommended (8 GB+ for local AI models)
- **Storage:** 4 GB minimum, 20 GB recommended
- **GPU:** Optional (NVIDIA CUDA or AMD ROCm for accelerated inference)

## Components

- **aios-init** — PID 1 init daemon with service supervision
- **aios-runtime** — AI inference runtime (llama.cpp integration)
- **aios-orchestrator** — Multi-agent goal decomposition and orchestration
- **aios-tools** — System tool execution sandbox
- **aios-memory** — Persistent and working memory with vector search
- **aios-api-gateway** — Management console and REST API (port 9090)

## Known Issues

- First boot may take several minutes if downloading models over the network
- GPU detection requires appropriate drivers in the rootfs

## Changelog

<!-- Fill in changes for this release -->
- Initial release of aiOS distribution
- Full installer with BIOS + EFI support
- First-boot initialization sequence
- AI-native service supervision
NOTES

ok "  Generated RELEASE-NOTES.md"

# -----------------------------------------------------------
# Step 5: Create release archive
# -----------------------------------------------------------
info "[5/5] Creating release archive..."

ARCHIVE_NAME="${RELEASE_NAME}-release.tar.gz"

tar czf "${RELEASE_DIR}/${ARCHIVE_NAME}" \
    -C "$(dirname "$RELEASE_DIR")" \
    "$(basename "$RELEASE_DIR")" \
    --exclude="${ARCHIVE_NAME}"

ok "  Created: ${ARCHIVE_NAME}"

# Update the SHA256SUMS to include the archive
(
    cd "$RELEASE_DIR"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$ARCHIVE_NAME" >> SHA256SUMS
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$ARCHIVE_NAME" >> SHA256SUMS
    fi
)

# -----------------------------------------------------------
# Release summary
# -----------------------------------------------------------
echo ""
echo ""
ok "========================================================"
ok " Release ${VERSION_TAG} packaged successfully"
ok "========================================================"
echo ""
info "Release directory: ${RELEASE_DIR}/"
echo ""
info "Contents:"

TOTAL_SIZE=0
for artifact in "${RELEASE_DIR}"/*; do
    [ -f "$artifact" ] || continue
    artifact_name="$(basename "$artifact")"
    artifact_size="$(du -h "$artifact" | cut -f1)"
    artifact_bytes="$(stat -f%z "$artifact" 2>/dev/null || stat -c%s "$artifact" 2>/dev/null || echo 0)"
    TOTAL_SIZE=$((TOTAL_SIZE + artifact_bytes))
    printf '  %-45s %s\n' "$artifact_name" "$artifact_size"
done

echo ""
if [ "$TOTAL_SIZE" -ge 1073741824 ]; then
    TOTAL_HUMAN="$(echo "scale=1; $TOTAL_SIZE / 1073741824" | bc 2>/dev/null || echo '?') GB"
elif [ "$TOTAL_SIZE" -ge 1048576 ]; then
    TOTAL_HUMAN="$(echo "scale=1; $TOTAL_SIZE / 1048576" | bc 2>/dev/null || echo '?') MB"
else
    TOTAL_HUMAN="${TOTAL_SIZE} bytes"
fi
info "Total release size: ${TOTAL_HUMAN}"
echo ""
ok "Checksums in: ${RELEASE_DIR}/SHA256SUMS"
ok "Release notes: ${RELEASE_DIR}/RELEASE-NOTES.md"
ok "Archive: ${RELEASE_DIR}/${ARCHIVE_NAME}"
echo ""
ok "========================================================"
