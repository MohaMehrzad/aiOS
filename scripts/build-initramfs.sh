#!/usr/bin/env bash
# ============================================================
# build-initramfs.sh — Create the aiOS early-boot initramfs
# ============================================================
# Produces:
#   build/output/initramfs.img   — gzipped cpio archive
#
# The initramfs:
#   1. Mounts proc, sys, devtmpfs
#   2. Waits for the root block device to appear
#   3. Mounts root read-only
#   4. Performs switch_root to /usr/sbin/aios-init
#
# Idempotent: safe to re-run; previous initramfs tree is rebuilt.
# ============================================================
set -euo pipefail

# -----------------------------------------------------------
# Resolve project root
# -----------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# -----------------------------------------------------------
# Constants
# -----------------------------------------------------------
BUSYBOX_VERSION="1.36.1"
BUSYBOX_URL="https://busybox.net/downloads/binaries/${BUSYBOX_VERSION}-defconfig-multiarch-musl/busybox-x86_64"
INITRAMFS_DIR="build/initramfs"
OUTPUT_DIR="build/output"
BUSYBOX_BIN="build/cache/busybox-${BUSYBOX_VERSION}-x86_64"

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[initramfs]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[initramfs]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[initramfs]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[initramfs]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Preflight
# -----------------------------------------------------------
for tool in cpio gzip wget; do
    command -v "$tool" >/dev/null 2>&1 || die "Required tool not found: $tool"
done

# -----------------------------------------------------------
# Step 1 — Download BusyBox static binary
# -----------------------------------------------------------
mkdir -p build/cache

if [ ! -f "$BUSYBOX_BIN" ]; then
    info "Downloading BusyBox ${BUSYBOX_VERSION} (static, x86_64)..."
    wget -q --show-progress -O "$BUSYBOX_BIN" "$BUSYBOX_URL"
    chmod +x "$BUSYBOX_BIN"
else
    info "BusyBox binary already cached."
fi

# Verify it is a real static binary (not an HTML error page)
if ! file "$BUSYBOX_BIN" | grep -qi 'elf.*executable'; then
    die "Downloaded BusyBox does not appear to be a valid ELF binary. Remove build/cache/ and retry."
fi

# -----------------------------------------------------------
# Step 2 — Create initramfs directory tree
# -----------------------------------------------------------
info "Creating initramfs directory tree..."
rm -rf "$INITRAMFS_DIR"
mkdir -p "${INITRAMFS_DIR}"/{bin,sbin,etc,proc,sys,dev,mnt/root,lib,lib64,run,tmp}

# Install BusyBox and create applet symlinks
cp "$BUSYBOX_BIN" "${INITRAMFS_DIR}/bin/busybox"
chmod 755 "${INITRAMFS_DIR}/bin/busybox"

# Create symlinks for the shell and essential applets
for applet in sh ash mount umount switch_root sleep cat grep tr cut echo mkdir mknod; do
    ln -sf busybox "${INITRAMFS_DIR}/bin/${applet}"
done

# -----------------------------------------------------------
# Step 3 — Create the init script
# -----------------------------------------------------------
info "Writing /init script..."

cat > "${INITRAMFS_DIR}/init" << 'INITSCRIPT'
#!/bin/sh
# ==========================================================
# aiOS initramfs /init
# ==========================================================
# Purpose: minimal early userspace that hands off to the real
#          init on the root filesystem.
# ==========================================================

export PATH=/bin:/sbin

# ----------------------------------------------------------
# Mount kernel virtual filesystems
# ----------------------------------------------------------
mount -t proc     proc     /proc
mount -t sysfs    sysfs    /sys
mount -t devtmpfs devtmpfs /dev

# Create a few device nodes that devtmpfs may not provide
# immediately.
mkdir -p /dev/pts /dev/shm
mount -t devpts devpts /dev/pts
mount -t tmpfs  tmpfs  /dev/shm

echo ""
echo "  aiOS initramfs"
echo "  =============="
echo ""

# ----------------------------------------------------------
# Parse kernel command line
# ----------------------------------------------------------
ROOT_DEV=""
INIT_BIN="/usr/sbin/aios-init"
ROOT_FLAGS="ro"

for param in $(cat /proc/cmdline); do
    case "$param" in
        root=*)
            ROOT_DEV="${param#root=}"
            ;;
        init=*)
            INIT_BIN="${param#init=}"
            ;;
        ro)
            ROOT_FLAGS="ro"
            ;;
        rw)
            ROOT_FLAGS="rw"
            ;;
    esac
done

if [ -z "$ROOT_DEV" ]; then
    echo "FATAL: no root= parameter on kernel command line"
    echo "Dropping to emergency shell..."
    exec /bin/sh
fi

echo "Root device : $ROOT_DEV"
echo "Init binary : $INIT_BIN"
echo "Mount flags : $ROOT_FLAGS"
echo ""

# ----------------------------------------------------------
# Wait for root device to appear (up to 30 seconds)
# ----------------------------------------------------------
echo "Waiting for root device..."
TRIES=0
MAX_TRIES=300

while [ ! -b "$ROOT_DEV" ] && [ "$TRIES" -lt "$MAX_TRIES" ]; do
    sleep 0.1
    TRIES=$((TRIES + 1))
done

if [ ! -b "$ROOT_DEV" ]; then
    echo "FATAL: root device $ROOT_DEV did not appear after 30 seconds"
    echo "Available block devices:"
    ls -la /dev/vd* /dev/sd* /dev/nvme* 2>/dev/null || echo "  (none)"
    echo ""
    echo "Dropping to emergency shell..."
    exec /bin/sh
fi

echo "Root device found after $((TRIES / 10)).$((TRIES % 10))s"

# ----------------------------------------------------------
# Mount root filesystem
# ----------------------------------------------------------
echo "Mounting root filesystem..."
if ! mount -o "$ROOT_FLAGS" "$ROOT_DEV" /mnt/root; then
    echo "FATAL: failed to mount $ROOT_DEV on /mnt/root"
    echo "Dropping to emergency shell..."
    exec /bin/sh
fi

# ----------------------------------------------------------
# Verify the init binary exists on root
# ----------------------------------------------------------
if [ ! -x "/mnt/root${INIT_BIN}" ]; then
    echo "WARNING: ${INIT_BIN} not found or not executable on root filesystem"
    echo "Contents of /mnt/root/usr/sbin/:"
    ls -la /mnt/root/usr/sbin/ 2>/dev/null || echo "  (empty or missing)"
    echo ""
    echo "Dropping to emergency shell (root is mounted at /mnt/root)..."
    exec /bin/sh
fi

# ----------------------------------------------------------
# Clean up and switch_root
# ----------------------------------------------------------
echo "Switching root to ${INIT_BIN}..."

# Unmount virtual filesystems — switch_root needs them gone
umount /dev/pts  2>/dev/null || true
umount /dev/shm  2>/dev/null || true
umount /proc     2>/dev/null || true
umount /sys      2>/dev/null || true
umount /dev      2>/dev/null || true

# switch_root moves the mount, deletes initramfs contents,
# chroots, and execs the target init.
exec switch_root /mnt/root "$INIT_BIN"
INITSCRIPT

chmod 755 "${INITRAMFS_DIR}/init"

# -----------------------------------------------------------
# Step 4 — Pack the initramfs
# -----------------------------------------------------------
mkdir -p "$OUTPUT_DIR"

info "Creating cpio+gzip archive..."
(
    cd "$INITRAMFS_DIR"
    find . -print0 | cpio --null -o -H newc --quiet 2>/dev/null | gzip -9 > "${PROJECT_ROOT}/${OUTPUT_DIR}/initramfs.img"
)

# -----------------------------------------------------------
# Step 5 — Summary
# -----------------------------------------------------------
IMG_SIZE="$(du -h "${OUTPUT_DIR}/initramfs.img" | cut -f1)"
FILE_COUNT="$(find "$INITRAMFS_DIR" -type f | wc -l | tr -d ' ')"

echo ""
ok "============================================"
ok " Initramfs build complete"
ok "============================================"
ok " Image:  ${OUTPUT_DIR}/initramfs.img  (${IMG_SIZE})"
ok " Files:  ${FILE_COUNT}"
ok "============================================"
