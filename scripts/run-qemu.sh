#!/usr/bin/env bash
# ============================================================
# run-qemu.sh — Launch aiOS in QEMU
# ============================================================
# Boots aiOS with virtio devices, port forwarding, and optional
# debug support.
#
# Usage:
#   ./scripts/run-qemu.sh              # Normal boot
#   ./scripts/run-qemu.sh --debug      # Wait for GDB on :1234
#   ./scripts/run-qemu.sh --graphic    # Show VGA window
#   ./scripts/run-qemu.sh --tap        # Use TAP networking
#   ./scripts/run-qemu.sh --iso        # Boot from ISO instead
#   ./scripts/run-qemu.sh --cpus 8 --ram 8G
#
# Port forwards (user-mode networking):
#   Host 9090  → Guest 9090  (management console)
#   Host 50051 → Guest 50051 (gRPC: orchestrator)
#   Host 50052 → Guest 50052 (gRPC: tools)
#   Host 50053 → Guest 50053 (gRPC: memory)
#   Host 50054 → Guest 50054 (gRPC: api-gateway)
# ============================================================
set -euo pipefail

# -----------------------------------------------------------
# Resolve project root
# -----------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# -----------------------------------------------------------
# Defaults
# -----------------------------------------------------------
OUTPUT_DIR="build/output"
KERNEL="${OUTPUT_DIR}/vmlinuz"
INITRAMFS="${OUTPUT_DIR}/initramfs.img"
ROOTFS="${OUTPUT_DIR}/rootfs.img"
ISO="${OUTPUT_DIR}/aios.iso"

CPUS=4
RAM="4G"
DEBUG=false
GRAPHIC=false
USE_TAP=false
BOOT_ISO=false
EXTRA_ARGS=()

# -----------------------------------------------------------
# Parse arguments
# -----------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug)
            DEBUG=true
            shift
            ;;
        --graphic)
            GRAPHIC=true
            shift
            ;;
        --tap)
            USE_TAP=true
            shift
            ;;
        --iso)
            BOOT_ISO=true
            shift
            ;;
        --cpus)
            CPUS="$2"
            shift 2
            ;;
        --ram)
            RAM="$2"
            shift 2
            ;;
        --kernel)
            KERNEL="$2"
            shift 2
            ;;
        --rootfs)
            ROOTFS="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --debug       Wait for GDB connection on tcp::1234"
            echo "  --graphic     Show QEMU VGA window (default: serial only)"
            echo "  --tap         Use TAP networking instead of user-mode"
            echo "  --iso         Boot from ISO image instead of kernel+rootfs"
            echo "  --cpus N      Number of virtual CPUs (default: 4)"
            echo "  --ram SIZE    Memory size (default: 4G)"
            echo "  --kernel PATH Path to kernel image"
            echo "  --rootfs PATH Path to rootfs image"
            echo ""
            echo "Port forwards (user-mode networking):"
            echo "  9090  → management console"
            echo "  50051 → gRPC orchestrator"
            echo "  50052 → gRPC tools"
            echo "  50053 → gRPC memory"
            echo "  50054 → gRPC api-gateway"
            exit 0
            ;;
        *)
            EXTRA_ARGS+=("$1")
            shift
            ;;
    esac
done

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[qemu]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[qemu]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[qemu]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[qemu]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Preflight
# -----------------------------------------------------------
QEMU="qemu-system-x86_64"
command -v "$QEMU" >/dev/null 2>&1 || die "qemu-system-x86_64 not found. Install QEMU."

# -----------------------------------------------------------
# Build QEMU command
# -----------------------------------------------------------
QEMU_CMD=("$QEMU")

# ---- Machine / CPU ----
# Acceleration: try KVM (Linux), then HVF (macOS), then TCG (software).
# With TCG we cannot use -cpu host, so detect the situation.
ACCEL=""
if [ -e /dev/kvm ] && [ -w /dev/kvm ]; then
    ACCEL="kvm"
    QEMU_CMD+=(-machine q35,accel=kvm)
    QEMU_CMD+=(-cpu host)
elif command -v sysctl >/dev/null 2>&1 && sysctl -n kern.hv_support 2>/dev/null | grep -q 1; then
    ACCEL="hvf"
    QEMU_CMD+=(-machine q35,accel=hvf)
    QEMU_CMD+=(-cpu host)
else
    ACCEL="tcg"
    QEMU_CMD+=(-machine q35,accel=tcg)
    QEMU_CMD+=(-cpu qemu64)
    warn "No hardware acceleration available — using TCG (slow)."
fi
info "Acceleration: ${ACCEL}"

QEMU_CMD+=(-smp "$CPUS")
QEMU_CMD+=(-m "$RAM")

# ---- Boot mode ----
if [ "$BOOT_ISO" = true ]; then
    # Boot from ISO
    if [ ! -f "$ISO" ]; then
        die "ISO not found: $ISO (run build-iso.sh first)"
    fi
    info "Booting from ISO: $ISO"
    QEMU_CMD+=(-cdrom "$ISO")
    QEMU_CMD+=(-boot d)
else
    # Direct kernel boot with rootfs
    if [ ! -f "$KERNEL" ]; then
        die "Kernel not found: $KERNEL (run build-kernel.sh first)"
    fi
    if [ ! -f "$ROOTFS" ]; then
        die "Root filesystem not found: $ROOTFS (run build-rootfs.sh first)"
    fi

    info "Kernel:    $KERNEL"
    info "Rootfs:    $ROOTFS"

    QEMU_CMD+=(-kernel "$KERNEL")
    QEMU_CMD+=(-append "root=/dev/vda console=ttyS0,115200 loglevel=4 init=/usr/sbin/aios-init")

    # Attach initramfs if present
    if [ -f "$INITRAMFS" ]; then
        info "Initramfs: $INITRAMFS"
        QEMU_CMD+=(-initrd "$INITRAMFS")
    else
        warn "No initramfs found — booting without it (root must be directly mountable)"
    fi

    # Root filesystem as virtio block device
    QEMU_CMD+=(
        -drive "file=${ROOTFS},format=raw,if=none,id=rootdisk"
        -device virtio-blk-pci,drive=rootdisk
    )
fi

# ---- Virtio devices ----
QEMU_CMD+=(-device virtio-serial-pci)
QEMU_CMD+=(-device virtio-rng-pci)

# ---- Networking ----
if [ "$USE_TAP" = true ]; then
    # TAP networking — requires root / pre-configured bridge
    info "Network: TAP mode (br0)"
    QEMU_CMD+=(
        -netdev tap,id=net0,ifname=tap-aios,script=no,downscript=no
        -device virtio-net-pci,netdev=net0,mac=52:54:00:12:34:56
    )
else
    # User-mode networking with port forwards
    info "Network: user mode with port forwarding"
    NETDEV="user,id=net0"
    NETDEV+=",hostfwd=tcp::9090-:9090"     # Management console
    NETDEV+=",hostfwd=tcp::50051-:50051"   # gRPC: orchestrator
    NETDEV+=",hostfwd=tcp::50052-:50052"   # gRPC: tools
    NETDEV+=",hostfwd=tcp::50053-:50053"   # gRPC: memory
    NETDEV+=",hostfwd=tcp::50054-:50054"   # gRPC: api-gateway
    NETDEV+=",hostname=aios"

    QEMU_CMD+=(
        -netdev "$NETDEV"
        -device virtio-net-pci,netdev=net0,mac=52:54:00:12:34:56
    )
fi

# ---- Display and serial console ----
# -nographic redirects serial0 to stdio and disables VGA, so we
# must NOT also add -serial when using it.
if [ "$GRAPHIC" = true ]; then
    info "Display: VGA window + serial on stdio"
    QEMU_CMD+=(-serial mon:stdio)
else
    info "Display: serial console only (use --graphic for VGA)"
    QEMU_CMD+=(-nographic)
fi

# ---- Debug ----
if [ "$DEBUG" = true ]; then
    info "Debug: GDB server on tcp::1234 — VM paused at start"
    QEMU_CMD+=(-s -S)
    echo ""
    ok "Connect GDB with:"
    ok "  gdb vmlinux -ex 'target remote :1234'"
    echo ""
fi

# ---- Misc ----
QEMU_CMD+=(-rtc base=utc,clock=host)

# Append any extra user-supplied arguments
if [ ${#EXTRA_ARGS[@]} -gt 0 ]; then
    QEMU_CMD+=("${EXTRA_ARGS[@]}")
fi

# -----------------------------------------------------------
# Launch
# -----------------------------------------------------------
echo ""
ok "============================================"
ok " Launching aiOS in QEMU"
ok "============================================"
ok " CPUs:   ${CPUS}"
ok " RAM:    ${RAM}"
ok " Debug:  ${DEBUG}"
if [ "$USE_TAP" = false ]; then
    ok ""
    ok " Port forwards:"
    ok "   localhost:9090  → management console"
    ok "   localhost:50051 → gRPC orchestrator"
    ok "   localhost:50052 → gRPC tools"
    ok "   localhost:50053 → gRPC memory"
    ok "   localhost:50054 → gRPC api-gateway"
fi
ok "============================================"
echo ""
info "Press Ctrl+A then X to exit QEMU."
echo ""

exec "${QEMU_CMD[@]}"
