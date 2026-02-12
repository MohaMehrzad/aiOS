# Phase 12: Distribution Packaging

## Goal
Package everything into a bootable ISO image that can be installed on bare metal or run in a VM. Create the installer, build pipeline, and release process.

## Prerequisites
- ALL previous phases complete and tested
- System boots and operates autonomously in QEMU

---

## Step-by-Step

### Step 12.1: Create Build Pipeline

**Claude Code prompt**: "Create a complete build pipeline script that compiles all components, assembles the rootfs, builds the kernel, downloads models, and produces a bootable disk image"

```bash
#!/bin/bash
# build/build-all.sh
set -euo pipefail

echo "=== aiOS Build Pipeline ==="
echo "Building all components..."

# 1. Build Rust components (parallel)
echo "[1/7] Building Rust workspace..."
cargo build --workspace --release --target x86_64-unknown-linux-musl

# 2. Build kernel
echo "[2/7] Building kernel..."
./kernel/scripts/build-kernel.sh

# 3. Build initramfs
echo "[3/7] Building initramfs..."
./kernel/scripts/build-initramfs.sh

# 4. Build llama.cpp
echo "[4/7] Building llama.cpp..."
./build/build-llamacpp.sh

# 5. Download models (skip if already present)
echo "[5/7] Checking models..."
./build/download-models.sh

# 6. Assemble rootfs
echo "[6/7] Assembling root filesystem..."
./build/build-rootfs.sh

# 7. Create disk image
echo "[7/7] Creating disk image..."
./build/create-disk-image.sh

echo "=== Build Complete ==="
echo "Disk image: build/output/aios.img"
echo "ISO: build/output/aios.iso (if ISO build enabled)"
```

### Step 12.2: Create ISO Builder

**Claude Code prompt**: "Create a script that packages the disk image into a bootable ISO with GRUB, suitable for burning to USB or mounting in a VM"

```bash
#!/bin/bash
# build/build-iso.sh
set -euo pipefail

ISO_DIR="build/iso-staging"
OUTPUT="build/output/aios.iso"

rm -rf "$ISO_DIR"
mkdir -p "$ISO_DIR"/{boot/grub,aios}

# Copy kernel and initramfs
cp build/output/vmlinuz "$ISO_DIR/boot/"
cp build/output/initramfs.img "$ISO_DIR/boot/"

# Copy rootfs image (will be installed to disk)
cp build/output/aios-rootfs.tar.gz "$ISO_DIR/aios/"

# Copy installer script
cp build/installer/install.sh "$ISO_DIR/aios/"

# Create GRUB config for ISO
cat > "$ISO_DIR/boot/grub/grub.cfg" << 'GRUB'
set timeout=5
set default=0

menuentry "Install aiOS" {
    linux /boot/vmlinuz console=ttyS0,115200 console=tty0 installer=1
    initrd /boot/initramfs.img
}

menuentry "aiOS Live (RAM)" {
    linux /boot/vmlinuz console=ttyS0,115200 console=tty0 live=1
    initrd /boot/initramfs.img
}
GRUB

# Build ISO
grub-mkrescue -o "$OUTPUT" "$ISO_DIR" \
    --modules="normal linux ext2 part_msdos" \
    -- -volid "AIOS"

echo "ISO created: $OUTPUT"
ls -lh "$OUTPUT"
```

### Step 12.3: Create Installer

**Claude Code prompt**: "Create an AI-assisted installer that detects target hardware, partitions the disk, copies the rootfs, installs GRUB, and configures the system"

```bash
#!/bin/bash
# build/installer/install.sh
# This runs inside the aiOS live environment

echo "==================================="
echo "   aiOS Installer v1.0"
echo "==================================="

# 1. Detect available disks
echo "Detecting disks..."
DISKS=$(lsblk -d -n -o NAME,SIZE,TYPE | grep disk)
echo "$DISKS"
echo ""

# 2. Select target disk
# In AI mode: the installer AI picks the best disk
# In manual mode: user selects
echo "Select target disk for installation:"
# ... disk selection logic ...

TARGET="/dev/$SELECTED_DISK"

# 3. Partition disk
echo "Partitioning $TARGET..."
parted -s "$TARGET" mklabel gpt
parted -s "$TARGET" mkpart primary fat32 1MiB 512MiB    # EFI
parted -s "$TARGET" set 1 esp on
parted -s "$TARGET" mkpart primary ext4 512MiB 100%     # Root

# 4. Format partitions
mkfs.fat -F32 "${TARGET}p1"   # or ${TARGET}1
mkfs.ext4 "${TARGET}p2"

# 5. Mount and extract rootfs
mount "${TARGET}p2" /mnt
mkdir -p /mnt/boot/efi
mount "${TARGET}p1" /mnt/boot/efi

echo "Extracting aiOS..."
tar xzf /cdrom/aios/aios-rootfs.tar.gz -C /mnt/

# 6. Install kernel and bootloader
cp /cdrom/boot/vmlinuz /mnt/boot/
cp /cdrom/boot/initramfs.img /mnt/boot/
grub-install --target=x86_64-efi --efi-directory=/mnt/boot/efi --boot-directory=/mnt/boot "$TARGET"

# Create grub.cfg for installed system
cat > /mnt/boot/grub/grub.cfg << GRUB
set timeout=0
set default=0
menuentry "aiOS" {
    linux /boot/vmlinuz root=${TARGET}p2 ro quiet console=ttyS0,115200 loglevel=3 init=/usr/sbin/aios-init
    initrd /boot/initramfs.img
}
GRUB

# 7. First-boot configuration
echo "Configuring first boot..."
# Generate unique hostname
HOSTNAME="aios-$(head -c 4 /dev/urandom | xxd -p)"
echo "$HOSTNAME" > /mnt/etc/hostname
sed -i "s/hostname = .*/hostname = \"$HOSTNAME\"/" /mnt/etc/aios/config.toml

# Generate SSH host keys
ssh-keygen -A -f /mnt

# Prompt for API keys
echo ""
echo "Enter your Claude API key (or press Enter to skip):"
read CLAUDE_KEY
if [ -n "$CLAUDE_KEY" ]; then
    echo "CLAUDE_API_KEY=$CLAUDE_KEY" >> /mnt/etc/aios/secrets.env
fi

echo "Enter your OpenAI API key (or press Enter to skip):"
read OPENAI_KEY
if [ -n "$OPENAI_KEY" ]; then
    echo "OPENAI_API_KEY=$OPENAI_KEY" >> /mnt/etc/aios/secrets.env
fi

# Encrypt secrets file using openssl (AES-256-GCM with argon2-derived key)
if [ -f /mnt/etc/aios/secrets.env ]; then
    echo ""
    echo "Set a passphrase to encrypt your API keys (remember this!):"
    read -s PASSPHRASE
    echo "$PASSPHRASE" | openssl enc -aes-256-cbc -salt -pbkdf2 -iter 100000 \
        -in /mnt/etc/aios/secrets.env \
        -out /mnt/etc/aios/secrets.enc
    # Delete plaintext — encrypted version is all that remains
    shred -u /mnt/etc/aios/secrets.env
    echo "Secrets encrypted to /etc/aios/secrets.enc"
    echo "At first boot, aios-init will prompt for the passphrase,"
    echo "load secrets into the kernel keyring, then delete secrets.enc."
fi

# 8. Cleanup
umount /mnt/boot/efi
umount /mnt

echo "==================================="
echo "   aiOS Installation Complete!"
echo "   Remove installation media and reboot."
echo "==================================="
```

### Step 12.4: Create First-Boot Experience

**Claude Code prompt**: "Implement the first-boot sequence in aios-init — generate crypto keys, encrypt secrets, detect hardware, configure networking, load models, and enter autonomy mode"

```
First Boot Sequence:
1. Normal boot phases 1-4
2. Detect: "This is first boot" (no /var/lib/aios/initialized flag)
3. Generate Ed25519 keypair for system identity
4. Encrypt secrets.env → secrets.enc (delete plaintext)
5. Initialize all databases (working memory, long-term, audit ledger)
6. Network Agent: detect and configure networking
7. Test API connectivity (if keys provided)
8. Download latest model files (if not included in ISO)
9. Run full system health check
10. Create /var/lib/aios/initialized
11. Enter autonomy mode
12. Log: "aiOS first boot complete. System is autonomous."
```

### Step 12.5: Create Management Console (Basic)

**Claude Code prompt**: "Create a minimal web-based management console — a simple HTTP API + static HTML page that shows system status, active goals, and allows submitting new goals"

```
Management Console:
  GET  /api/status     → System status, agent health, resource usage
  GET  /api/goals      → List all goals and their status
  POST /api/goals      → Submit a new goal
  GET  /api/audit      → Recent audit log entries
  GET  /api/budget     → API usage and budget status
  POST /api/emergency  → Emergency stop (halt all agents)

  GET  /               → Static HTML dashboard
```

### Step 12.6: Documentation and Release Notes

**Claude Code prompt**: "Create user-facing documentation — installation guide, first-boot guide, management console guide, and troubleshooting"

### Step 12.7: Final Integration Test

**Claude Code prompt**: "Full end-to-end test: build ISO, install in QEMU, first boot, verify all systems come online, submit a goal via management console, verify goal completes"

Test scenario:
1. Build ISO from clean checkout
2. Create QEMU VM with blank disk
3. Boot from ISO, run installer
4. Reboot into installed system
5. Verify all services start (runtime, orchestrator, tools, memory, agents)
6. Access management console on port 9090
7. Submit goal: "Check system health and report"
8. Verify goal completes with accurate report
9. Submit goal: "Install and configure nginx as a web server"
10. Verify nginx is installed and running

---

## Deliverables Checklist

- [ ] `build/build-all.sh` builds everything from source
- [ ] `build/build-iso.sh` creates bootable ISO
- [ ] Installer partitions, formats, and installs to disk
- [ ] First-boot sequence completes successfully
- [ ] Management console accessible on port 9090
- [ ] System enters autonomy mode after first boot
- [ ] Full end-to-end test passes (ISO → install → boot → goal → success)
- [ ] User documentation written
- [ ] ISO size is reasonable (<2GB without models, <10GB with models)

---

## You Did It!

If all 12 phases are complete and tested, you have a working AI-native operating system. Next steps:
- Harden for production use
- Add more agent capabilities
- Fine-tune local models on system-specific tasks
- Multi-node support
- Community release
