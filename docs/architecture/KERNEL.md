# Kernel Architecture

## Overview

aiOS uses a custom-compiled Linux 6.x kernel stripped to the minimum required for AI workloads. We remove everything a human-operated desktop needs (sound, Bluetooth, USB HID, display managers) and optimize for what AI needs (fast I/O, GPU compute, networking, containers).

---

## Kernel Version

**Target: Linux 6.8.x (LTS)** — Stable, well-supported, has all GPU and filesystem features we need.

Source: https://kernel.org

---

## Configuration Strategy

Start from `tinyconfig`, then add only what we need. This produces a kernel under 10MB.

### Enabled (Required)

```
# Core
CONFIG_64BIT=y
CONFIG_SMP=y                          # Multi-core support
CONFIG_PREEMPT_VOLUNTARY=y            # Good balance for server workloads
CONFIG_HIGH_RES_TIMERS=y
CONFIG_NUMA=y                         # For multi-socket servers
CONFIG_CGROUPS=y                      # Container support
CONFIG_NAMESPACES=y                   # Container isolation

# Filesystem
CONFIG_EXT4_FS=y                      # Root filesystem
CONFIG_XFS_FS=y                       # Data/model storage (better for large files)
CONFIG_TMPFS=y                        # /tmp, /run
CONFIG_PROC_FS=y                      # /proc
CONFIG_SYSFS=y                        # /sys
CONFIG_DEVTMPFS=y                     # /dev
CONFIG_FUSE_FS=y                      # User-space filesystems
CONFIG_OVERLAY_FS=y                   # Container layers

# Block devices
CONFIG_BLK_DEV_NVME=y                 # NVMe SSD support
CONFIG_BLK_DEV_SD=y                   # SCSI/SATA drives
CONFIG_ATA=y
CONFIG_SATA_AHCI=y

# Networking
CONFIG_NET=y
CONFIG_INET=y                         # IPv4
CONFIG_IPV6=y                         # IPv6
CONFIG_NETFILTER=y                    # Firewall (iptables/nftables)
CONFIG_NF_CONNTRACK=y
CONFIG_NF_NAT=y
CONFIG_BRIDGE=y                       # Container networking
CONFIG_VETH=y                         # Virtual ethernet pairs
CONFIG_TUN=y                          # VPN support
CONFIG_WIREGUARD=y                    # Modern VPN

# Drivers
CONFIG_E1000E=y                       # Intel Ethernet
CONFIG_VIRTIO_NET=y                   # QEMU/KVM network
CONFIG_VIRTIO_BLK=y                   # QEMU/KVM storage
CONFIG_VIRTIO_PCI=y                   # QEMU/KVM PCI
CONFIG_NET_VENDOR_REALTEK=y           # Common NIC

# GPU (as modules — load only if present)
CONFIG_DRM=m
CONFIG_DRM_NOUVEAU=m                  # NVIDIA open (fallback)
CONFIG_DRM_AMDGPU=m                   # AMD GPU

# Security
CONFIG_SECCOMP=y                      # Syscall filtering
CONFIG_SECURITY=y
CONFIG_SECURITY_APPARMOR=y            # Application sandboxing
CONFIG_AUDIT=y                        # System audit logging
CONFIG_KEYS=y                         # Kernel keyring

# Container support
CONFIG_VETH=y
CONFIG_BRIDGE_NETFILTER=y
CONFIG_NETFILTER_XT_MATCH_CONNTRACK=y
CONFIG_IP_NF_NAT=y
CONFIG_OVERLAY_FS=y
CONFIG_USER_NS=y                      # User namespaces
CONFIG_PID_NS=y
CONFIG_NET_NS=y
CONFIG_UTS_NS=y
CONFIG_IPC_NS=y

# Performance
CONFIG_NO_HZ_FULL=y                   # Tickless kernel for compute
CONFIG_TRANSPARENT_HUGEPAGE=y         # Better memory for AI models
CONFIG_KSM=y                          # Kernel same-page merging
CONFIG_ZRAM=y                         # Compressed swap
```

### Disabled (Stripped)

```
# Human interface (not needed)
CONFIG_SOUND=n                        # No audio
CONFIG_INPUT_MOUSE=n                  # No mouse
CONFIG_INPUT_KEYBOARD=n               # No keyboard (serial console only)
CONFIG_HID=n                          # No human interface devices
CONFIG_USB_HID=n
CONFIG_BT=n                           # No Bluetooth
CONFIG_WIRELESS=n                     # No WiFi (use Ethernet)

# Display (not needed)
CONFIG_VGA_CONSOLE=n                  # No VGA
CONFIG_FB=n                           # No framebuffer
CONFIG_DRM_FBDEV_EMULATION=n

# Other unnecessary
CONFIG_SWAP=n                         # We manage memory ourselves (or use ZRAM)
CONFIG_PROFILING=n
CONFIG_DEBUG_INFO=n                   # Strips debug symbols (smaller kernel)
```

---

## NVIDIA GPU Support

NVIDIA proprietary drivers are required for CUDA. These CANNOT be compiled into the kernel and must be loaded as modules post-boot.

### Strategy
1. Build kernel with `CONFIG_DRM=m` and `CONFIG_MODULES=y`
2. Include NVIDIA driver installer in the root filesystem
3. `aios-init` detects NVIDIA GPU at boot → installs/loads driver
4. The AI Runtime (llama.cpp) then detects CUDA and uses it

### Driver Installation (automated in Phase 2)
```bash
# Included in rootfs as /usr/src/nvidia-driver.run
# aios-init runs this at boot if NVIDIA GPU detected
/usr/src/nvidia-driver.run --silent --no-questions
modprobe nvidia
modprobe nvidia_uvm
```

### Alternative: Use nouveau (open-source)
Works for basic GPU tasks but NO CUDA support. Only use as fallback.

---

## Boot Configuration

### GRUB Config (`/boot/grub/grub.cfg`)
```
set timeout=0
set default=0

menuentry "aiOS" {
    linux /boot/vmlinuz root=/dev/sda1 ro quiet console=ttyS0,115200 loglevel=3 init=/usr/sbin/aios-init
    initrd /boot/initramfs.img
}
```

Key kernel parameters:
- `quiet` — Suppress boot messages (AI doesn't need them)
- `console=ttyS0,115200` — Serial console for debug/management
- `loglevel=3` — Only errors
- `init=/usr/sbin/aios-init` — Our custom init, not systemd

### Initramfs
Minimal initramfs that:
1. Loads essential drivers (NVMe, ext4)
2. Mounts root filesystem
3. Switches root to the real filesystem
4. Exec's `/usr/sbin/aios-init`

Built with a custom script, NOT dracut or mkinitramfs (too bloated).

---

## Kernel Build Process

```bash
# 1. Download kernel source
wget https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.8.12.tar.xz
tar xf linux-6.8.12.tar.xz
cd linux-6.8.12

# 2. Start from tinyconfig
make tinyconfig

# 3. Apply our config overlay
scripts/kconfig/merge_config.sh .config /path/to/aios-kernel.config

# 4. Build
make -j$(nproc)

# 5. Output
# Kernel: arch/x86/boot/bzImage (~8-10MB)
# Modules: in respective directories
make modules_install INSTALL_MOD_PATH=/tmp/aios-modules

# 6. Install to build directory
cp arch/x86/boot/bzImage /path/to/aios-build/boot/vmlinuz
```

---

## Kernel Patches (Optional, Phase 2+)

### Patch 1: AI Scheduler Class
Custom scheduling class that gives priority to AI inference processes. Not required for MVP but improves performance.

```
SCHED_AI priority class:
- Higher priority than SCHED_NORMAL
- Lower than SCHED_FIFO (we don't want to starve the system)
- Automatic detection of llama.cpp/inference processes
```

### Patch 2: Memory Pressure Notifications
Enhanced memory pressure notifications that the AI can subscribe to, allowing proactive memory management rather than reactive OOM killing.

### Patch 3: GPU Memory Telemetry
Expose GPU memory stats through /proc/gpu_memory for the AI to monitor without needing nvidia-smi.

---

## Kernel Module Loading Strategy

```
Boot (aios-init handles module loading):
1. Always load:
   - Storage: nvme, ahci, ext4, xfs
   - Network: virtio_net (VM) or e1000e/realtek (bare metal)
   - Security: apparmor

2. Load on detection:
   - GPU: nvidia (if NVIDIA card found in lspci)
   - GPU: amdgpu (if AMD card found in lspci)
   - Network: additional drivers based on detected hardware

3. Never load:
   - Sound, HID, Bluetooth, wireless
```

---

## Testing the Kernel

### QEMU Boot Test
```bash
qemu-system-x86_64 \
    -kernel /path/to/vmlinuz \
    -initrd /path/to/initramfs.img \
    -append "console=ttyS0 root=/dev/sda1" \
    -drive file=rootfs.img,format=raw \
    -nographic \
    -m 4G \
    -smp 4 \
    -enable-kvm
```

### Validation Checklist
- [ ] Boots to serial console in <5 seconds
- [ ] All filesystems mount correctly
- [ ] Network interface comes up
- [ ] GPU detected (if present)
- [ ] `/proc` and `/sys` populated
- [ ] PID 1 is `aios-init`
- [ ] No unnecessary modules loaded
- [ ] Kernel size under 15MB
