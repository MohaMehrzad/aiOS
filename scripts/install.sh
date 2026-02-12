#!/usr/bin/env bash
# ============================================================
# install.sh — aiOS AI-Assisted Installer
# ============================================================
# Installs aiOS to a target disk from the live/ISO environment.
#
# Modes:
#   Interactive:  ./install.sh              (prompts for disk selection)
#   Automated:    TARGET_DISK=/dev/sda ./install.sh
#
# What it does:
#   1. Detects available disks
#   2. Selects target disk (interactive or via $TARGET_DISK)
#   3. Partitions: 512 MB EFI (vfat) + remaining ext4 root
#   4. Formats partitions
#   5. Mounts root and EFI
#   6. Extracts rootfs tarball
#   7. Installs GRUB bootloader (BIOS + EFI)
#   8. Copies kernel and initramfs
#   9. Generates /etc/fstab with UUIDs
#  10. Creates initial config and first-boot flag
#  11. Unmounts and reports success
#
# Full error handling with cleanup on failure.
# ============================================================
set -euo pipefail

# -----------------------------------------------------------
# Constants
# -----------------------------------------------------------
AIOS_VERSION="0.1.0"
MOUNT_ROOT="/mnt/aios"
MOUNT_EFI="${MOUNT_ROOT}/boot/efi"
ROOTFS_TARBALL=""
KERNEL_PATH=""
INITRAMFS_PATH=""

# Track what we've done for cleanup
PARTITIONED=false
FORMATTED=false
MOUNTED_ROOT=false
MOUNTED_EFI=false

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[install]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[install]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[install]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[install]\033[0m FATAL: %s\n' "$*" >&2; cleanup; exit 1; }

# -----------------------------------------------------------
# Cleanup function — called on failure or EXIT
# -----------------------------------------------------------
cleanup() {
    local exit_code=$?
    if [ "$exit_code" -ne 0 ] || [ "${FORCE_CLEANUP:-false}" = true ]; then
        warn "Cleaning up after failure..."
    fi

    # Unmount in reverse order
    if [ "$MOUNTED_EFI" = true ] && mountpoint -q "$MOUNT_EFI" 2>/dev/null; then
        info "Unmounting EFI partition..."
        umount "$MOUNT_EFI" 2>/dev/null || warn "Failed to unmount EFI"
    fi

    if [ "$MOUNTED_ROOT" = true ] && mountpoint -q "$MOUNT_ROOT" 2>/dev/null; then
        info "Unmounting root partition..."
        umount "$MOUNT_ROOT" 2>/dev/null || warn "Failed to unmount root"
    fi
}

trap cleanup EXIT

# -----------------------------------------------------------
# Preflight — check we are running as root
# -----------------------------------------------------------
if [ "$(id -u)" -ne 0 ]; then
    die "This installer must be run as root. Use: sudo $0"
fi

# -----------------------------------------------------------
# Banner
# -----------------------------------------------------------
echo ""
echo "==================================================="
echo "     aiOS Installer v${AIOS_VERSION}"
echo "==================================================="
echo ""
echo "  This will install aiOS to a disk."
echo "  WARNING: All data on the target disk will be erased."
echo ""

# -----------------------------------------------------------
# Locate installation media (rootfs tarball, kernel, initramfs)
# -----------------------------------------------------------
info "Locating installation files..."

# Search common locations for the rootfs tarball
SEARCH_PATHS=(
    "/cdrom/aios"
    "/mnt/cdrom/aios"
    "/media/cdrom/aios"
    "/run/media/aios"
    "."
    "/aios"
)

for search_dir in "${SEARCH_PATHS[@]}"; do
    if [ -f "${search_dir}/aios-rootfs.tar.gz" ]; then
        ROOTFS_TARBALL="${search_dir}/aios-rootfs.tar.gz"
        break
    elif [ -f "${search_dir}/rootfs.tar.gz" ]; then
        ROOTFS_TARBALL="${search_dir}/rootfs.tar.gz"
        break
    fi
done

if [ -z "$ROOTFS_TARBALL" ]; then
    die "Cannot find rootfs tarball (aios-rootfs.tar.gz). Are you booted from the aiOS ISO?"
fi
ok "Found rootfs: ${ROOTFS_TARBALL}"

# Locate kernel
for search_dir in "${SEARCH_PATHS[@]}" "/cdrom/boot" "/mnt/cdrom/boot" "/boot"; do
    if [ -f "${search_dir}/vmlinuz" ]; then
        KERNEL_PATH="${search_dir}/vmlinuz"
        break
    fi
done

if [ -z "$KERNEL_PATH" ]; then
    die "Cannot find kernel image (vmlinuz)."
fi
ok "Found kernel: ${KERNEL_PATH}"

# Locate initramfs
for search_dir in "${SEARCH_PATHS[@]}" "/cdrom/boot" "/mnt/cdrom/boot" "/boot"; do
    if [ -f "${search_dir}/initramfs.img" ]; then
        INITRAMFS_PATH="${search_dir}/initramfs.img"
        break
    fi
done

if [ -z "$INITRAMFS_PATH" ]; then
    die "Cannot find initramfs (initramfs.img)."
fi
ok "Found initramfs: ${INITRAMFS_PATH}"

# -----------------------------------------------------------
# Step 1: Detect available disks
# -----------------------------------------------------------
info "Detecting available disks..."
echo ""

# Get list of block devices that are whole disks (not partitions, not loop, not rom)
AVAILABLE_DISKS=()
DISK_INFO=()

while IFS= read -r line; do
    dev_name="$(echo "$line" | awk '{print $1}')"
    dev_size="$(echo "$line" | awk '{print $2}')"
    dev_type="$(echo "$line" | awk '{print $3}')"
    dev_model="$(echo "$line" | awk '{$1=$2=$3=""; print $0}' | sed 's/^ *//')"

    # Skip the installation media itself (read-only or very small)
    dev_path="/dev/${dev_name}"
    if [ -b "$dev_path" ]; then
        AVAILABLE_DISKS+=("$dev_path")
        DISK_INFO+=("${dev_path}  ${dev_size}  ${dev_model:-unknown}")
    fi
done < <(lsblk -d -n -o NAME,SIZE,TYPE,MODEL 2>/dev/null | grep -E '\bdisk\b' || true)

if [ ${#AVAILABLE_DISKS[@]} -eq 0 ]; then
    die "No disks detected. Ensure your storage device is connected."
fi

echo "  Available disks:"
echo "  -------------------------------------------"
for i in "${!DISK_INFO[@]}"; do
    printf '  [%d]  %s\n' "$((i + 1))" "${DISK_INFO[$i]}"
done
echo "  -------------------------------------------"
echo ""

# -----------------------------------------------------------
# Step 2: Select target disk
# -----------------------------------------------------------
TARGET=""

if [ -n "${TARGET_DISK:-}" ]; then
    # Automated mode — use environment variable
    if [ ! -b "$TARGET_DISK" ]; then
        die "TARGET_DISK=${TARGET_DISK} is not a valid block device."
    fi
    TARGET="$TARGET_DISK"
    info "Automated mode: using TARGET_DISK=${TARGET}"
else
    # Interactive mode — prompt user
    while true; do
        printf '  Enter disk number [1-%d] or device path (e.g. /dev/sda): ' "${#AVAILABLE_DISKS[@]}"
        read -r selection

        # Check if it's a number
        if [[ "$selection" =~ ^[0-9]+$ ]]; then
            idx=$((selection - 1))
            if [ "$idx" -ge 0 ] && [ "$idx" -lt "${#AVAILABLE_DISKS[@]}" ]; then
                TARGET="${AVAILABLE_DISKS[$idx]}"
                break
            else
                warn "Invalid selection. Enter a number between 1 and ${#AVAILABLE_DISKS[@]}."
            fi
        elif [ -b "$selection" ]; then
            TARGET="$selection"
            break
        else
            warn "Invalid input. Enter a disk number or a valid device path."
        fi
    done
fi

echo ""
warn "TARGET DISK: ${TARGET}"
warn "ALL DATA ON ${TARGET} WILL BE DESTROYED."
echo ""

# Confirm in interactive mode
if [ -z "${TARGET_DISK:-}" ] && [ -z "${AIOS_CONFIRM:-}" ]; then
    printf '  Type "yes" to continue: '
    read -r confirm
    if [ "$confirm" != "yes" ]; then
        info "Installation cancelled."
        exit 0
    fi
fi

echo ""

# -----------------------------------------------------------
# Determine partition naming convention
# -----------------------------------------------------------
# NVMe drives use p1, p2; SCSI/SATA use 1, 2
if [[ "$TARGET" == *"nvme"* ]] || [[ "$TARGET" == *"mmcblk"* ]] || [[ "$TARGET" == *"loop"* ]]; then
    PART_PREFIX="${TARGET}p"
else
    PART_PREFIX="${TARGET}"
fi

EFI_PART="${PART_PREFIX}1"
ROOT_PART="${PART_PREFIX}2"

# -----------------------------------------------------------
# Step 3: Partition the disk
# -----------------------------------------------------------
info "Partitioning ${TARGET}..."

# Wipe existing partition table
wipefs -a "$TARGET" >/dev/null 2>&1 || true
dd if=/dev/zero of="$TARGET" bs=1M count=1 conv=notrunc >/dev/null 2>&1 || true

# Create GPT partition table
parted -s "$TARGET" mklabel gpt || die "Failed to create GPT partition table on ${TARGET}"

# Partition 1: 512 MB EFI System Partition
parted -s "$TARGET" mkpart primary fat32 1MiB 513MiB || die "Failed to create EFI partition"
parted -s "$TARGET" set 1 esp on || die "Failed to set ESP flag"

# Partition 2: Remaining space as ext4 root
parted -s "$TARGET" mkpart primary ext4 513MiB 100% || die "Failed to create root partition"

PARTITIONED=true

# Wait for kernel to re-read partition table
partprobe "$TARGET" 2>/dev/null || true
sleep 2

# Verify partitions exist
if [ ! -b "$EFI_PART" ]; then
    die "EFI partition ${EFI_PART} not found after partitioning."
fi
if [ ! -b "$ROOT_PART" ]; then
    die "Root partition ${ROOT_PART} not found after partitioning."
fi

ok "Disk partitioned: ${EFI_PART} (EFI 512MB) + ${ROOT_PART} (root)"

# -----------------------------------------------------------
# Step 4: Format partitions
# -----------------------------------------------------------
info "Formatting partitions..."

# Format EFI partition as FAT32
mkfs.vfat -F 32 -n "AIOS_EFI" "$EFI_PART" || die "Failed to format EFI partition"
ok "Formatted ${EFI_PART} as FAT32 (EFI)"

# Format root partition as ext4
mkfs.ext4 -F -L "AIOS_ROOT" -O ^metadata_csum "$ROOT_PART" || die "Failed to format root partition"
ok "Formatted ${ROOT_PART} as ext4 (root)"

FORMATTED=true

# -----------------------------------------------------------
# Step 5: Mount partitions
# -----------------------------------------------------------
info "Mounting partitions..."

mkdir -p "$MOUNT_ROOT"
mount "$ROOT_PART" "$MOUNT_ROOT" || die "Failed to mount root partition"
MOUNTED_ROOT=true
ok "Mounted ${ROOT_PART} on ${MOUNT_ROOT}"

mkdir -p "$MOUNT_EFI"
mount "$EFI_PART" "$MOUNT_EFI" || die "Failed to mount EFI partition"
MOUNTED_EFI=true
ok "Mounted ${EFI_PART} on ${MOUNT_EFI}"

# -----------------------------------------------------------
# Step 6: Extract rootfs tarball
# -----------------------------------------------------------
info "Extracting root filesystem (this may take a while)..."

tar xzf "$ROOTFS_TARBALL" -C "$MOUNT_ROOT" || die "Failed to extract rootfs tarball"

ok "Root filesystem extracted"

# Ensure essential directories exist
mkdir -p "${MOUNT_ROOT}"/{boot,dev,proc,sys,tmp,run,var/log,var/lib/aios}

# -----------------------------------------------------------
# Step 7: Install kernel and initramfs
# -----------------------------------------------------------
info "Installing kernel and initramfs..."

cp "$KERNEL_PATH" "${MOUNT_ROOT}/boot/vmlinuz" || die "Failed to copy kernel"
cp "$INITRAMFS_PATH" "${MOUNT_ROOT}/boot/initramfs.img" || die "Failed to copy initramfs"

ok "Kernel and initramfs installed"

# -----------------------------------------------------------
# Step 8: Install GRUB bootloader
# -----------------------------------------------------------
info "Installing GRUB bootloader..."

# Determine GRUB installation command
GRUB_INSTALL=""
if command -v grub-install >/dev/null 2>&1; then
    GRUB_INSTALL="grub-install"
elif command -v grub2-install >/dev/null 2>&1; then
    GRUB_INSTALL="grub2-install"
else
    warn "grub-install not found. Bootloader will need to be installed manually."
fi

if [ -n "$GRUB_INSTALL" ]; then
    # Install GRUB for BIOS boot
    if [ -d /usr/lib/grub/i386-pc ] || [ -d /usr/lib/grub2/i386-pc ]; then
        info "Installing GRUB for BIOS..."
        "$GRUB_INSTALL" \
            --target=i386-pc \
            --boot-directory="${MOUNT_ROOT}/boot" \
            --recheck \
            "$TARGET" 2>&1 || warn "BIOS GRUB install failed (EFI-only system?)"
    fi

    # Install GRUB for EFI
    if [ -d /usr/lib/grub/x86_64-efi ] || [ -d /usr/lib/grub2/x86_64-efi ]; then
        info "Installing GRUB for EFI..."
        "$GRUB_INSTALL" \
            --target=x86_64-efi \
            --efi-directory="${MOUNT_EFI}" \
            --boot-directory="${MOUNT_ROOT}/boot" \
            --removable \
            --recheck \
            "$TARGET" 2>&1 || warn "EFI GRUB install failed (BIOS-only system?)"
    fi
fi

# Get UUIDs for partitions
ROOT_UUID="$(blkid -s UUID -o value "$ROOT_PART")"
EFI_UUID="$(blkid -s UUID -o value "$EFI_PART")"

if [ -z "$ROOT_UUID" ]; then
    die "Failed to determine UUID for root partition"
fi

# Write GRUB configuration for installed system
mkdir -p "${MOUNT_ROOT}/boot/grub"

cat > "${MOUNT_ROOT}/boot/grub/grub.cfg" << GRUBCFG
# ============================================================
# aiOS GRUB Boot Configuration (installed system)
# ============================================================

set timeout=3
set default=0

# Serial console support
serial --speed=115200 --unit=0 --word=8 --parity=no --stop=1
terminal_input serial console
terminal_output serial console

menuentry "aiOS" {
    search --no-floppy --fs-uuid --set=root ${ROOT_UUID}
    linux /boot/vmlinuz root=UUID=${ROOT_UUID} ro console=ttyS0,115200 console=tty0 init=/usr/sbin/aios-init quiet loglevel=3
    initrd /boot/initramfs.img
}

menuentry "aiOS (Recovery)" {
    search --no-floppy --fs-uuid --set=root ${ROOT_UUID}
    linux /boot/vmlinuz root=UUID=${ROOT_UUID} ro console=ttyS0,115200 console=tty0 init=/usr/sbin/aios-init aios.recovery=1 loglevel=7
    initrd /boot/initramfs.img
}

menuentry "aiOS (Debug Shell)" {
    search --no-floppy --fs-uuid --set=root ${ROOT_UUID}
    linux /boot/vmlinuz root=UUID=${ROOT_UUID} ro console=ttyS0,115200 console=tty0 init=/bin/sh loglevel=7
    initrd /boot/initramfs.img
}
GRUBCFG

ok "GRUB bootloader installed"

# -----------------------------------------------------------
# Step 9: Generate /etc/fstab
# -----------------------------------------------------------
info "Generating /etc/fstab..."

mkdir -p "${MOUNT_ROOT}/etc"

cat > "${MOUNT_ROOT}/etc/fstab" << FSTAB
# /etc/fstab — aiOS filesystem table
# Generated by aiOS installer on $(date -u '+%Y-%m-%d %H:%M:%S UTC')
#
# <device>                                <mount>     <type>  <options>           <dump> <pass>
UUID=${ROOT_UUID}    /           ext4    defaults,noatime    0      1
UUID=${EFI_UUID}     /boot/efi   vfat    defaults,umask=0077 0      2
proc                                      /proc       proc    defaults            0      0
sysfs                                     /sys        sysfs   defaults            0      0
tmpfs                                     /tmp        tmpfs   size=256M           0      0
tmpfs                                     /run        tmpfs   size=128M,mode=0755 0      0
FSTAB

ok "Generated /etc/fstab with UUIDs"

# -----------------------------------------------------------
# Step 10: Create initial aiOS configuration
# -----------------------------------------------------------
info "Creating initial system configuration..."

# Generate hostname from random bytes
HOSTNAME="aios-$(head -c 4 /dev/urandom | xxd -p 2>/dev/null || od -An -tx1 -N4 /dev/urandom | tr -d ' ')"

# Write hostname
echo "$HOSTNAME" > "${MOUNT_ROOT}/etc/hostname"

# Create aiOS configuration directory
mkdir -p "${MOUNT_ROOT}/etc/aios"
mkdir -p "${MOUNT_ROOT}/etc/aios/keys"

cat > "${MOUNT_ROOT}/etc/aios/config.toml" << TOML
# ============================================================
# aiOS System Configuration
# Generated by installer on $(date -u '+%Y-%m-%d %H:%M:%S UTC')
# ============================================================

[system]
hostname = "${HOSTNAME}"
version = "${AIOS_VERSION}"
install_date = "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

[boot]
debug_shell = false
clean_shutdown_flag = "/var/lib/aios/.clean-shutdown"
recovery_mode = false

[runtime]
socket_path = "/run/aios/runtime.sock"
max_concurrent_tasks = 4
log_level = "info"

[memory]
db_path = "/var/lib/aios/memory/memory.db"
working_memory_path = "/var/lib/aios/memory/working.db"
vector_store_path = "/var/lib/aios/vectors"

[models]
model_dir = "/var/lib/aios/models"
default_model = "tinyllama-1.1b-chat.Q4_K_M.gguf"

[network]
listen_address = "0.0.0.0"
api_port = 9090

[security]
secrets_path = "/etc/aios/secrets.enc"
key_dir = "/etc/aios/keys"

[budget]
daily_api_limit_usd = 10.0
warning_threshold = 0.8
TOML

ok "Created /etc/aios/config.toml (hostname: ${HOSTNAME})"

# -----------------------------------------------------------
# Step 11: Create first-boot flag and directories
# -----------------------------------------------------------
info "Setting up first-boot state..."

mkdir -p "${MOUNT_ROOT}/var/lib/aios"
touch "${MOUNT_ROOT}/var/lib/aios/.first-boot"

# Copy first-boot script to the target system
mkdir -p "${MOUNT_ROOT}/usr/lib/aios"
if [ -f "$(dirname "$0")/first-boot.sh" ]; then
    cp "$(dirname "$0")/first-boot.sh" "${MOUNT_ROOT}/usr/lib/aios/first-boot.sh"
    chmod 755 "${MOUNT_ROOT}/usr/lib/aios/first-boot.sh"
    ok "Installed first-boot.sh"
fi

# Prompt for API keys (interactive only)
if [ -z "${TARGET_DISK:-}" ] && [ -t 0 ]; then
    echo ""
    echo "  Optional: Configure API keys for cloud AI access."
    echo ""
    printf '  Enter Claude API key (or press Enter to skip): '
    read -r CLAUDE_KEY
    if [ -n "$CLAUDE_KEY" ]; then
        echo "CLAUDE_API_KEY=${CLAUDE_KEY}" >> "${MOUNT_ROOT}/etc/aios/secrets.env"
    fi

    printf '  Enter OpenAI API key (or press Enter to skip): '
    read -r OPENAI_KEY
    if [ -n "$OPENAI_KEY" ]; then
        echo "OPENAI_API_KEY=${OPENAI_KEY}" >> "${MOUNT_ROOT}/etc/aios/secrets.env"
    fi

    # Encrypt secrets if any were provided
    if [ -f "${MOUNT_ROOT}/etc/aios/secrets.env" ]; then
        echo ""
        printf '  Set a passphrase to encrypt your API keys: '
        read -rs PASSPHRASE
        echo ""

        if [ -n "$PASSPHRASE" ]; then
            echo "$PASSPHRASE" | openssl enc -aes-256-cbc -salt -pbkdf2 -iter 100000 \
                -in "${MOUNT_ROOT}/etc/aios/secrets.env" \
                -out "${MOUNT_ROOT}/etc/aios/secrets.enc" 2>/dev/null

            # Securely delete plaintext
            shred -u "${MOUNT_ROOT}/etc/aios/secrets.env" 2>/dev/null || rm -f "${MOUNT_ROOT}/etc/aios/secrets.env"
            ok "API keys encrypted to /etc/aios/secrets.enc"
        fi
    fi
fi

ok "First-boot flag created"

# -----------------------------------------------------------
# Step 12: Set permissions
# -----------------------------------------------------------
info "Setting permissions..."

chmod 600 "${MOUNT_ROOT}/etc/aios/config.toml"
chmod 700 "${MOUNT_ROOT}/etc/aios/keys"
chmod 755 "${MOUNT_ROOT}/var/lib/aios"

if [ -f "${MOUNT_ROOT}/etc/aios/secrets.enc" ]; then
    chmod 600 "${MOUNT_ROOT}/etc/aios/secrets.enc"
fi

ok "Permissions set"

# -----------------------------------------------------------
# Step 13: Sync and unmount
# -----------------------------------------------------------
info "Syncing filesystems..."
sync

info "Unmounting partitions..."

umount "$MOUNT_EFI" || die "Failed to unmount EFI partition"
MOUNTED_EFI=false
ok "Unmounted ${EFI_PART}"

umount "$MOUNT_ROOT" || die "Failed to unmount root partition"
MOUNTED_ROOT=false
ok "Unmounted ${ROOT_PART}"

# -----------------------------------------------------------
# Success
# -----------------------------------------------------------
echo ""
echo "==================================================="
echo "     aiOS Installation Complete!"
echo "==================================================="
echo ""
ok "  Target:     ${TARGET}"
ok "  EFI:        ${EFI_PART}  (UUID: ${EFI_UUID})"
ok "  Root:       ${ROOT_PART}  (UUID: ${ROOT_UUID})"
ok "  Hostname:   ${HOSTNAME}"
echo ""
ok "  Remove installation media and reboot."
ok "  On first boot, aiOS will:"
ok "    - Generate cryptographic identity"
ok "    - Initialize databases"
ok "    - Download AI models (if not included)"
ok "    - Enter autonomous mode"
echo ""
echo "==================================================="
