#!/usr/bin/env bash
# ============================================================
# build-iso.sh — Build a bootable aiOS ISO image
# ============================================================
# Produces:
#   build/output/aios.iso — GRUB-bootable hybrid ISO (BIOS+EFI)
#
# Prerequisites (in build/output/):
#   vmlinuz         — kernel image
#   initramfs.img   — initial ramdisk
#   rootfs.img      — root filesystem disk image
#
# Idempotent: re-running replaces the ISO from current artifacts.
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
OUTPUT_DIR="build/output"
ISO_STAGING="build/iso-staging"
ISO_OUTPUT="${OUTPUT_DIR}/aios.iso"

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[iso]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[iso]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[iso]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[iso]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Preflight — check that build artifacts exist
# -----------------------------------------------------------
info "Checking build prerequisites..."

MISSING=0
for artifact in vmlinuz initramfs.img rootfs.img; do
    if [ ! -f "${OUTPUT_DIR}/${artifact}" ]; then
        warn "Missing: ${OUTPUT_DIR}/${artifact}"
        MISSING=$((MISSING + 1))
    fi
done

if [ "$MISSING" -gt 0 ]; then
    die "${MISSING} required artifact(s) missing. Run build-kernel.sh, build-initramfs.sh, and build-rootfs.sh first."
fi

# Check for ISO-building tool
MKRESCUE=""
if command -v grub-mkrescue >/dev/null 2>&1; then
    MKRESCUE="grub-mkrescue"
elif command -v grub2-mkrescue >/dev/null 2>&1; then
    MKRESCUE="grub2-mkrescue"
elif command -v xorriso >/dev/null 2>&1; then
    MKRESCUE="xorriso"
else
    die "No ISO builder found. Install grub-mkrescue (grub2-common) or xorriso."
fi
info "Using ISO builder: ${MKRESCUE}"

# -----------------------------------------------------------
# Step 1 — Create ISO staging directory
# -----------------------------------------------------------
info "Creating ISO staging directory..."
rm -rf "$ISO_STAGING"
mkdir -p "${ISO_STAGING}"/{boot/grub,aios,EFI/BOOT}

# -----------------------------------------------------------
# Step 2 — Copy boot artifacts
# -----------------------------------------------------------
info "Copying kernel and initramfs..."
cp "${OUTPUT_DIR}/vmlinuz"       "${ISO_STAGING}/boot/vmlinuz"
cp "${OUTPUT_DIR}/initramfs.img" "${ISO_STAGING}/boot/initramfs.img"

info "Copying root filesystem image..."
cp "${OUTPUT_DIR}/rootfs.img"    "${ISO_STAGING}/aios/rootfs.img"

# -----------------------------------------------------------
# Step 3 — Create GRUB configuration
# -----------------------------------------------------------
info "Writing GRUB configuration..."

cat > "${ISO_STAGING}/boot/grub/grub.cfg" << 'GRUBCFG'
# ============================================================
# aiOS GRUB Boot Configuration
# ============================================================

set timeout=5
set default=0

# Use serial console by default (with VGA fallback)
serial --speed=115200 --unit=0 --word=8 --parity=no --stop=1
terminal_input serial console
terminal_output serial console

menuentry "aiOS — Boot" {
    echo "Loading aiOS kernel..."
    linux /boot/vmlinuz root=/dev/vda console=ttyS0,115200 console=tty0 loglevel=4 init=/usr/sbin/aios-init
    echo "Loading initramfs..."
    initrd /boot/initramfs.img
}

menuentry "aiOS — Boot (verbose)" {
    echo "Loading aiOS kernel (verbose)..."
    linux /boot/vmlinuz root=/dev/vda console=ttyS0,115200 console=tty0 loglevel=7 init=/usr/sbin/aios-init earlyprintk=serial
    echo "Loading initramfs..."
    initrd /boot/initramfs.img
}

menuentry "aiOS — Boot (debug shell)" {
    echo "Loading aiOS kernel (rescue mode)..."
    linux /boot/vmlinuz root=/dev/vda console=ttyS0,115200 console=tty0 loglevel=7 init=/bin/sh
    echo "Loading initramfs..."
    initrd /boot/initramfs.img
}

menuentry "aiOS — Install to disk" {
    echo "Loading aiOS installer..."
    linux /boot/vmlinuz console=ttyS0,115200 console=tty0 loglevel=4 installer=1
    echo "Loading initramfs..."
    initrd /boot/initramfs.img
}
GRUBCFG

# -----------------------------------------------------------
# Step 4 — Build the ISO
# -----------------------------------------------------------
info "Building ISO image..."

if [ "$MKRESCUE" = "xorriso" ]; then
    # Direct xorriso invocation (when grub-mkrescue is not available)
    # Create a minimal El Torito boot image from the kernel
    xorriso -as mkisofs \
        -volid "AIOS" \
        -isohybrid-mbr /usr/lib/grub/i386-pc/boot_hybrid.img 2>/dev/null || true \
        -partition_offset 16 \
        -b boot/grub/i386-pc/eltorito.img \
        -no-emul-boot \
        -boot-load-size 4 \
        -boot-info-table \
        --grub2-boot-info \
        --grub2-mbr /usr/lib/grub/i386-pc/boot_hybrid.img \
        -o "$ISO_OUTPUT" \
        "$ISO_STAGING" 2>&1 || {
            # Fallback: simple ISO without El Torito (still usable with QEMU -cdrom)
            warn "El Torito boot image creation failed, building simple data ISO..."
            xorriso -as mkisofs \
                -volid "AIOS" \
                -r -J \
                -o "$ISO_OUTPUT" \
                "$ISO_STAGING"
        }
else
    # grub-mkrescue handles BIOS + EFI hybrid booting automatically
    "$MKRESCUE" \
        -o "$ISO_OUTPUT" \
        "$ISO_STAGING" \
        --modules="normal linux ext2 part_msdos part_gpt iso9660 biosdisk serial" \
        -- -volid "AIOS" 2>&1
fi

# -----------------------------------------------------------
# Step 5 — Verify output
# -----------------------------------------------------------
if [ ! -f "$ISO_OUTPUT" ]; then
    die "ISO creation failed — output file not found."
fi

# -----------------------------------------------------------
# Summary
# -----------------------------------------------------------
ISO_SIZE="$(du -h "$ISO_OUTPUT" | cut -f1)"
KERNEL_SIZE="$(du -h "${OUTPUT_DIR}/vmlinuz" | cut -f1)"
INITRAMFS_SIZE="$(du -h "${OUTPUT_DIR}/initramfs.img" | cut -f1)"
ROOTFS_SIZE="$(du -h "${OUTPUT_DIR}/rootfs.img" | cut -f1)"

echo ""
ok "============================================"
ok " ISO build complete"
ok "============================================"
ok " ISO:        ${ISO_OUTPUT}  (${ISO_SIZE})"
ok ""
ok " Contents:"
ok "   kernel:     ${KERNEL_SIZE}"
ok "   initramfs:  ${INITRAMFS_SIZE}"
ok "   rootfs:     ${ROOTFS_SIZE}"
ok "============================================"
echo ""
info "Boot with QEMU:"
info "  qemu-system-x86_64 -cdrom ${ISO_OUTPUT} -m 4G -nographic"
echo ""
info "Or use the run-qemu.sh script for full configuration."
