# Phase 2: Custom Kernel Build

## Goal
Build a minimal Linux kernel optimized for AI workloads. It should boot in QEMU in under 5 seconds and be under 15MB.

## Prerequisites
- Phase 1 complete (dev environment working)
- Read [architecture/KERNEL.md](../architecture/KERNEL.md) first

---

## Step-by-Step

### Step 2.1: Download Kernel Source

**Claude Code prompt**: "Download Linux kernel 6.8.x LTS source and extract it"

```bash
mkdir -p kernel/src
cd kernel/src
wget https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.8.12.tar.xz
tar xf linux-6.8.12.tar.xz
cd linux-6.8.12
```

### Step 2.2: Create Kernel Configuration

**Claude Code prompt**: "Create a minimal kernel config for aiOS starting from tinyconfig, adding NVMe, ext4, virtio, networking, cgroups, and namespaces"

Start from `tinyconfig` and add our requirements:

```bash
# Start minimal
make tinyconfig

# Then apply our overlay config
# Create kernel/configs/aios-kernel.config with the config from architecture/KERNEL.md
scripts/kconfig/merge_config.sh .config ../../configs/aios-kernel.config
```

The config file `kernel/configs/aios-kernel.config` should contain all the `CONFIG_*` entries from [architecture/KERNEL.md](../architecture/KERNEL.md).

### Step 2.3: Build the Kernel

**Claude Code prompt**: "Create a kernel build script that compiles the kernel and installs modules"

```bash
#!/bin/bash
# kernel/scripts/build-kernel.sh
set -euo pipefail

KERNEL_SRC="kernel/src/linux-6.8.12"
OUTPUT_DIR="build/output"

mkdir -p "$OUTPUT_DIR"

cd "$KERNEL_SRC"

# Build kernel image
make -j$(nproc)

# Copy kernel image
cp arch/x86/boot/bzImage "../../$OUTPUT_DIR/vmlinuz"

# Install modules to a temp directory
make modules_install INSTALL_MOD_PATH="../../$OUTPUT_DIR/modules"

echo "Kernel built successfully:"
ls -lh "../../$OUTPUT_DIR/vmlinuz"
echo "Modules installed to: $OUTPUT_DIR/modules"
```

### Step 2.4: Build Initramfs

**Claude Code prompt**: "Create a minimal initramfs that mounts the root filesystem and execs our init"

```bash
#!/bin/bash
# kernel/scripts/build-initramfs.sh
set -euo pipefail

INITRAMFS_DIR="build/initramfs"
OUTPUT_DIR="build/output"

# Clean and create structure
rm -rf "$INITRAMFS_DIR"
mkdir -p "$INITRAMFS_DIR"/{bin,sbin,etc,proc,sys,dev,mnt/root,lib,lib64}

# Copy BusyBox static binary (provides basic shell for debugging)
cp /usr/bin/busybox "$INITRAMFS_DIR/bin/" 2>/dev/null || \
    wget -O "$INITRAMFS_DIR/bin/busybox" \
    "https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox"
chmod +x "$INITRAMFS_DIR/bin/busybox"

# Create init script
cat > "$INITRAMFS_DIR/init" << 'INIT'
#!/bin/busybox sh

# Mount essential filesystems
/bin/busybox mount -t proc none /proc
/bin/busybox mount -t sysfs none /sys
/bin/busybox mount -t devtmpfs none /dev

# Parse kernel command line for root device
ROOT_DEV=$(cat /proc/cmdline | tr ' ' '\n' | grep '^root=' | cut -d= -f2)

# Wait for root device
echo "Waiting for root device: $ROOT_DEV"
while [ ! -b "$ROOT_DEV" ]; do
    /bin/busybox sleep 0.1
done

# Mount root filesystem
/bin/busybox mount -o ro "$ROOT_DEV" /mnt/root

# Switch root
exec /bin/busybox switch_root /mnt/root /usr/sbin/aios-init
INIT
chmod +x "$INITRAMFS_DIR/init"

# Create initramfs image
cd "$INITRAMFS_DIR"
find . | cpio -o -H newc | gzip > "../output/initramfs.img"

echo "Initramfs built:"
ls -lh "../output/initramfs.img"
```

### Step 2.5: Create Test Root Filesystem

**Claude Code prompt**: "Create a minimal root filesystem image with BusyBox and a stub aios-init for boot testing"

```bash
#!/bin/bash
# build/create-test-rootfs.sh
set -euo pipefail

IMG="build/qemu/rootfs.img"
MNT="build/qemu/mnt"

# Create 2GB disk image
dd if=/dev/zero of="$IMG" bs=1M count=2048

# Create partition and filesystem
parted -s "$IMG" mklabel msdos
parted -s "$IMG" mkpart primary ext4 1MiB 100%

# Set up loop device and format
LOOP=$(sudo losetup --find --show -P "$IMG")
sudo mkfs.ext4 "${LOOP}p1"

# Mount and populate
mkdir -p "$MNT"
sudo mount "${LOOP}p1" "$MNT"

# Create filesystem structure
sudo mkdir -p "$MNT"/{bin,sbin,etc,proc,sys,dev,tmp,var,run,usr/{bin,sbin,lib,share},home}

# Install BusyBox
sudo cp /usr/bin/busybox "$MNT/bin/"
sudo chroot "$MNT" /bin/busybox --install -s /bin

# Create stub aios-init (Phase 3 will replace this with real one)
sudo tee "$MNT/usr/sbin/aios-init" << 'INIT'
#!/bin/sh
echo "==================================="
echo "  aiOS Init v0.1 (stub)"
echo "==================================="
echo "Mounting filesystems..."
mount -t proc none /proc
mount -t sysfs none /sys
mount -t devtmpfs none /dev
mount -t tmpfs none /tmp
mount -t tmpfs none /run

echo "System booted successfully!"
echo "Dropping to shell (real init will be implemented in Phase 3)"
exec /bin/sh
INIT
sudo chmod +x "$MNT/usr/sbin/aios-init"

# Create minimal /etc files
echo "aios" | sudo tee "$MNT/etc/hostname"
echo "root:x:0:0:root:/root:/bin/sh" | sudo tee "$MNT/etc/passwd"

# Unmount
sudo umount "$MNT"
sudo losetup -d "$LOOP"

echo "Test rootfs created: $IMG"
```

### Step 2.6: Boot Test

**Claude Code prompt**: "Boot the kernel in QEMU and verify it reaches our init stub"

```bash
# Run QEMU
./build/run-qemu.sh build/output/vmlinuz build/qemu/rootfs.img

# Expected output:
# ... kernel boot messages ...
# ===================================
#   aiOS Init v0.1 (stub)
# ===================================
# Mounting filesystems...
# System booted successfully!
# Dropping to shell (real init will be implemented in Phase 3)
# / #
```

### Step 2.7: Measure and Optimize

```bash
# Kernel size (target: <15MB)
ls -lh build/output/vmlinuz

# Boot time (target: <5 seconds)
# Time from QEMU start to "System booted successfully!" message

# Module count (should be minimal)
find build/output/modules -name "*.ko" | wc -l
```

---

## Deliverables Checklist

- [ ] Kernel source downloaded and extracted
- [ ] `kernel/configs/aios-kernel.config` with our minimal config
- [ ] `kernel/scripts/build-kernel.sh` builds successfully
- [ ] `kernel/scripts/build-initramfs.sh` creates initramfs
- [ ] `build/create-test-rootfs.sh` creates a bootable rootfs
- [ ] System boots in QEMU to our init stub
- [ ] Kernel size under 15MB
- [ ] Boot time under 5 seconds
- [ ] Serial console works (can type commands)

---

## Troubleshooting

| Problem | Solution |
|---|---|
| Kernel panic: no init found | Check `init=` kernel parameter matches actual path |
| Cannot mount root | Check root device matches disk format (virtio vs IDE) |
| No serial output | Ensure `console=ttyS0` in kernel params + CONFIG_SERIAL_8250=y |
| KVM not available | Run `kvm-ok`, may need to enable in BIOS or use `-no-kvm` |
| Kernel too large | Check for debug symbols (CONFIG_DEBUG_INFO=n) |

---

## Next Phase
Once boot test passes → [Phase 3: Base System](./03-BASE-SYSTEM.md)
