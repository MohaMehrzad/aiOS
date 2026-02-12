# Phase 3: Base System (Minimal Userspace)

## Goal
Build a minimal but complete userspace with our Rust-based `aios-init` as PID 1, BusyBox for basic utilities, proper filesystem layout, and logging infrastructure.

## Prerequisites
- Phase 2 complete (kernel boots to stub init)
- Read [architecture/SYSTEM.md](../architecture/SYSTEM.md) — filesystem layout and boot sequence sections

---

## Step-by-Step

### Step 3.1: Implement `aios-init` (Rust)

**Claude Code prompt**: "Implement the aios-init Rust binary — PID 1 that mounts filesystems, sets up the environment, and will later start AI services. For now it should boot to a status message and spawn a debug shell."

```
File: initd/src/main.rs

Responsibilities:
1. Mount /proc, /sys, /dev, /tmp, /run (as tmpfs)
2. Set hostname from /etc/aios/config.toml (or default)
3. Set up basic environment variables
4. Initialize logging to /var/log/aios/init.log
5. Detect hardware (CPU cores, RAM, GPU presence via /proc and /sys)
6. Print system info banner
7. [Future] Start aios-runtime, aios-memory, aios-tools, aios-orchestrator
8. [For now] Spawn /bin/sh as debug console on serial
9. Reap zombie processes (PID 1 duty)
10. Handle SIGCHLD, SIGTERM, SIGINT
```

Key implementation details:
```rust
// initd/Cargo.toml dependencies
[dependencies]
nix = { version = "0.29", features = ["mount", "signal", "process"] }
toml = "0.8"
serde = { version = "1", features = ["derive"] }
log = "0.4"

// Must be statically linked for PID 1!
// Build with: cargo build --release --target x86_64-unknown-linux-musl
```

**Static linking is CRITICAL** — PID 1 runs before any shared libraries are available.

```toml
# initd/Cargo.toml
[profile.release]
opt-level = "s"        # Optimize for size
lto = true             # Link-time optimization
strip = true           # Strip debug symbols
```

### Step 3.2: Build Root Filesystem

**Claude Code prompt**: "Create a build script that assembles the full aiOS root filesystem with our init, BusyBox, proper directory structure, and config files"

```bash
#!/bin/bash
# build/build-rootfs.sh
set -euo pipefail

ROOTFS="build/rootfs"
OUTPUT="build/output"

# Clean and create structure
rm -rf "$ROOTFS"
mkdir -p "$ROOTFS"

# Create FHS-compliant directory tree
mkdir -p "$ROOTFS"/{bin,sbin,usr/{bin,sbin,lib,share}}
mkdir -p "$ROOTFS"/etc/{aios/{agents,tools,models},network,security}
mkdir -p "$ROOTFS"/var/{lib/aios/{models,memory,vectors,cache,ledger},log/aios,run/aios}
mkdir -p "$ROOTFS"/run/aios/agents
mkdir -p "$ROOTFS"/{proc,sys,dev,tmp,home/workspaces}

# 1. Install BusyBox (provides coreutils, shell, etc.)
install -m 755 /path/to/busybox-static "$ROOTFS/bin/busybox"
chroot "$ROOTFS" /bin/busybox --install -s /bin

# 2. Install our init
install -m 755 target/x86_64-unknown-linux-musl/release/aios-init "$ROOTFS/usr/sbin/aios-init"

# 3. Create default config
cat > "$ROOTFS/etc/aios/config.toml" << 'CONFIG'
[system]
hostname = "aios"
log_level = "info"
autonomy_level = "supervised"

[models]
runtime = "llama-cpp"
model_dir = "/var/lib/aios/models"
CONFIG

# 4. Create minimal system files
echo "aios" > "$ROOTFS/etc/hostname"
echo "root:x:0:0:root:/root:/bin/sh" > "$ROOTFS/etc/passwd"
echo "root:x:0:" > "$ROOTFS/etc/group"
echo "nameserver 1.1.1.1" > "$ROOTFS/etc/resolv.conf"

cat > "$ROOTFS/etc/fstab" << 'FSTAB'
# <device>  <mount>  <type>  <options>  <dump>  <pass>
proc        /proc    proc    defaults   0       0
sysfs       /sys     sysfs   defaults   0       0
devtmpfs    /dev     devtmpfs defaults  0       0
tmpfs       /tmp     tmpfs   defaults   0       0
tmpfs       /run     tmpfs   defaults   0       0
FSTAB

echo "Root filesystem assembled at $ROOTFS"
```

### Step 3.3: Create Disk Image from Rootfs

**Claude Code prompt**: "Create a script that takes the rootfs directory and produces a bootable disk image with GRUB"

```bash
#!/bin/bash
# build/create-disk-image.sh
set -euo pipefail

ROOTFS="build/rootfs"
IMG="build/output/aios.img"
SIZE_MB=2048

# Create disk image
dd if=/dev/zero of="$IMG" bs=1M count=$SIZE_MB

# Partition: BIOS boot + root
parted -s "$IMG" mklabel msdos
parted -s "$IMG" mkpart primary ext4 1MiB 100%
parted -s "$IMG" set 1 boot on

# Setup loop device
LOOP=$(sudo losetup --find --show -P "$IMG")
sudo mkfs.ext4 "${LOOP}p1"

# Mount and copy rootfs
MNT=$(mktemp -d)
sudo mount "${LOOP}p1" "$MNT"
sudo cp -a "$ROOTFS"/* "$MNT/"

# Install kernel
sudo mkdir -p "$MNT/boot"
sudo cp build/output/vmlinuz "$MNT/boot/vmlinuz"
sudo cp build/output/initramfs.img "$MNT/boot/initramfs.img"

# Install GRUB
sudo grub-install --target=i386-pc --boot-directory="$MNT/boot" "$LOOP"

# Create GRUB config
sudo tee "$MNT/boot/grub/grub.cfg" << 'GRUB'
set timeout=0
set default=0
menuentry "aiOS" {
    linux /boot/vmlinuz root=/dev/sda1 ro quiet console=ttyS0,115200 loglevel=3 init=/usr/sbin/aios-init
    initrd /boot/initramfs.img
}
GRUB

# Cleanup
sudo umount "$MNT"
sudo losetup -d "$LOOP"
rmdir "$MNT"

echo "Disk image created: $IMG"
```

### Step 3.4: Add Logging Infrastructure

**Claude Code prompt**: "Add basic logging to aios-init — write to /var/log/aios/init.log and serial console simultaneously"

The init should log:
- Timestamps for each boot phase
- Hardware detection results
- Mount operations
- Service starts (future)
- Errors and warnings

Format: `[2024-01-01T12:00:00Z] [INFO] [init] Message here`

### Step 3.5: Hardware Detection

**Claude Code prompt**: "Implement hardware detection in aios-init — read /proc/cpuinfo, /proc/meminfo, scan /sys/class for GPU"

The init should detect and report:
```
=== aiOS Hardware Report ===
CPU:     AMD EPYC 7742 (64 cores)
RAM:     128 GB
GPU:     NVIDIA A100 80GB (CUDA capable)
Storage: /dev/nvme0n1 (1 TB NVMe SSD)
Network: eth0 (Intel I210)
```

This info is stored in memory and used later by the orchestrator for resource management.

### Step 3.6: Boot Test

**Claude Code prompt**: "Build everything and boot test in QEMU — verify aios-init runs, mounts filesystems, detects hardware, and provides a debug shell"

```bash
# Build init (static binary)
cargo build --release --target x86_64-unknown-linux-musl -p aios-init

# Build rootfs
./build/build-rootfs.sh

# Create disk image
sudo ./build/create-disk-image.sh

# Boot test
./build/run-qemu.sh build/output/aios.img
```

Expected output:
```
[0.000] [INFO] [init] aiOS Init v0.1.0 starting...
[0.001] [INFO] [init] Mounting /proc...
[0.002] [INFO] [init] Mounting /sys...
[0.003] [INFO] [init] Mounting /dev...
[0.004] [INFO] [init] Mounting /tmp...
[0.005] [INFO] [init] Mounting /run...
[0.010] [INFO] [init] Loading configuration from /etc/aios/config.toml
[0.015] [INFO] [init] === aiOS Hardware Report ===
[0.015] [INFO] [init] CPU:     QEMU Virtual CPU (4 cores)
[0.015] [INFO] [init] RAM:     4096 MB
[0.015] [INFO] [init] GPU:     None detected
[0.015] [INFO] [init] Storage: /dev/vda (2 GB)
[0.015] [INFO] [init] Network: eth0
[0.020] [INFO] [init] Hostname set to: aios
[0.025] [INFO] [init] Phase 1 (Hardware) complete
[0.025] [WARN] [init] AI Runtime not yet implemented — spawning debug shell
[0.030] [INFO] [init] Debug shell available on serial console

/ #
```

---

## Deliverables Checklist

- [ ] `aios-init` Rust binary compiles as static musl binary
- [ ] `aios-init` mounts all required filesystems
- [ ] `aios-init` reads config from `/etc/aios/config.toml`
- [ ] `aios-init` detects and reports hardware
- [ ] `aios-init` sets hostname
- [ ] `aios-init` initializes logging
- [ ] `aios-init` reaps zombie processes
- [ ] `aios-init` handles signals gracefully
- [ ] `build/build-rootfs.sh` creates complete rootfs
- [ ] `build/create-disk-image.sh` creates bootable image with GRUB
- [ ] System boots in QEMU to debug shell
- [ ] Logs written to `/var/log/aios/init.log`
- [ ] All filesystems mounted correctly (`mount` shows correct mounts)

---

## Next Phase
Once boot test passes → [Phase 4: AI Runtime](./04-AI-RUNTIME.md)
