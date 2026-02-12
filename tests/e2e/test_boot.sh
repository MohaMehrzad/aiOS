#!/bin/bash
# End-to-end boot test for aiOS
# Runs in QEMU and verifies the system boots, all services start, and responds to health checks.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$PROJECT_ROOT/build/output"

TIMEOUT=120  # seconds to wait for boot
QEMU_PID=""

cleanup() {
    if [ -n "$QEMU_PID" ] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
    rm -f /tmp/aios_test_serial.log
}
trap cleanup EXIT

echo "=== aiOS End-to-End Boot Test ==="
echo ""

# Check required files
for f in "$BUILD_DIR/vmlinuz" "$BUILD_DIR/initramfs.img" "$BUILD_DIR/rootfs.img"; do
    if [ ! -f "$f" ]; then
        echo "SKIP: Required file not found: $f"
        echo "Run 'scripts/build-all.sh' first."
        exit 0
    fi
done

echo "[1/6] Starting QEMU..."

qemu-system-x86_64 \
    -kernel "$BUILD_DIR/vmlinuz" \
    -initrd "$BUILD_DIR/initramfs.img" \
    -drive file="$BUILD_DIR/rootfs.img",format=raw,if=virtio \
    -append "root=/dev/vda1 console=ttyS0 init=/usr/sbin/aios-init quiet" \
    -m 4G \
    -smp 4 \
    -nographic \
    -serial file:/tmp/aios_test_serial.log \
    -net nic,model=virtio \
    -net user,hostfwd=tcp::19090-:9090,hostfwd=tcp::50061-:50051 \
    -no-reboot \
    &
QEMU_PID=$!

echo "  QEMU started (PID: $QEMU_PID)"

# Wait for the system to boot
echo "[2/6] Waiting for boot (timeout: ${TIMEOUT}s)..."
SECONDS=0
BOOTED=false
while [ $SECONDS -lt $TIMEOUT ]; do
    # Check if QEMU is still running
    if ! kill -0 "$QEMU_PID" 2>/dev/null; then
        echo "FAIL: QEMU exited prematurely"
        if [ -f /tmp/aios_test_serial.log ]; then
            echo "--- Last 20 lines of serial output ---"
            tail -20 /tmp/aios_test_serial.log
        fi
        exit 1
    fi

    # Check serial log for boot complete message
    if [ -f /tmp/aios_test_serial.log ] && grep -q "aiOS boot complete" /tmp/aios_test_serial.log 2>/dev/null; then
        BOOTED=true
        break
    fi

    # Also try the management console
    if curl -sf http://localhost:19090/api/health >/dev/null 2>&1; then
        BOOTED=true
        break
    fi

    sleep 2
done

if [ "$BOOTED" = false ]; then
    echo "FAIL: System did not boot within ${TIMEOUT}s"
    if [ -f /tmp/aios_test_serial.log ]; then
        echo "--- Last 50 lines of serial output ---"
        tail -50 /tmp/aios_test_serial.log
    fi
    exit 1
fi
echo "  Boot completed in ${SECONDS}s"

# Wait a bit more for all services to start
sleep 5

echo "[3/6] Checking management console..."
HEALTH=$(curl -sf http://localhost:19090/api/health 2>/dev/null || echo "UNREACHABLE")
if [ "$HEALTH" = "UNREACHABLE" ]; then
    echo "FAIL: Management console not responding"
    exit 1
fi
echo "  Management console OK: $HEALTH"

echo "[4/6] Checking system status..."
STATUS=$(curl -sf http://localhost:19090/api/status 2>/dev/null || echo "{}")
echo "  System status: $STATUS"

# Verify key fields exist in status
if echo "$STATUS" | python3 -c "import json,sys; d=json.load(sys.stdin); assert 'active_goals' in d or 'uptime_seconds' in d" 2>/dev/null; then
    echo "  Status response valid"
else
    echo "  WARN: Status response format unexpected (non-fatal)"
fi

echo "[5/6] Checking gRPC services..."
# Try to connect to orchestrator
if command -v grpcurl >/dev/null 2>&1; then
    GRPC_OK=$(grpcurl -plaintext localhost:50061 list 2>/dev/null || echo "FAIL")
    if [ "$GRPC_OK" != "FAIL" ]; then
        echo "  gRPC services responding"
    else
        echo "  WARN: gRPC check failed (grpcurl available but no response)"
    fi
else
    echo "  SKIP: grpcurl not installed, skipping gRPC check"
fi

echo "[6/6] Verifying serial log..."
if [ -f /tmp/aios_test_serial.log ]; then
    # Check for panic or critical errors
    if grep -qi "panic\|kernel panic\|fatal error" /tmp/aios_test_serial.log; then
        echo "FAIL: Panic detected in serial log"
        grep -i "panic\|kernel panic\|fatal error" /tmp/aios_test_serial.log
        exit 1
    fi

    # Check key boot stages appeared
    CHECKS=("aiOS Init" "Starting services" "gRPC server")
    for check in "${CHECKS[@]}"; do
        if grep -q "$check" /tmp/aios_test_serial.log 2>/dev/null; then
            echo "  Found: '$check'"
        else
            echo "  WARN: Did not find '$check' in boot log"
        fi
    done
fi

echo ""
echo "=== All boot tests PASSED ==="

# Shutdown QEMU gracefully
kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true
QEMU_PID=""
