#!/usr/bin/env bash
# ============================================================
# build-rootfs.sh — Assemble the aiOS root filesystem image
# ============================================================
# Produces:
#   build/output/rootfs.img  — 2 GB ext4 disk image
#
# Requires sudo for loop-mount operations.
# Idempotent: re-running rebuilds the image from scratch.
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
ROOTFS_IMG="${OUTPUT_DIR}/rootfs.img"
ROOTFS_SIZE_MB=2048
MNT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aios-rootfs-mnt.XXXXXX")"
BUSYBOX_BIN="build/cache/busybox-1.36.1-x86_64"
BUSYBOX_URL="https://busybox.net/downloads/binaries/1.36.1-defconfig-multiarch-musl/busybox-x86_64"

# Rust release binaries (cross-compiled for x86_64 Linux)
RUST_TARGET_DIR="target/x86_64-unknown-linux-musl/release"
RUST_BINARIES=(aios-init aios-orchestrator aios-tools aios-memory aios-api-gateway)

# -----------------------------------------------------------
# Color helpers
# -----------------------------------------------------------
info()  { printf '\033[1;34m[rootfs]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[rootfs]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[rootfs]\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31m[rootfs]\033[0m %s\n' "$*" >&2; exit 1; }

# -----------------------------------------------------------
# Cleanup trap — always unmount and detach loop on exit
# -----------------------------------------------------------
LOOP_DEV=""
cleanup() {
    set +e
    if mountpoint -q "$MNT_DIR" 2>/dev/null; then
        sudo umount "$MNT_DIR"
    fi
    if [ -n "$LOOP_DEV" ]; then
        sudo losetup -d "$LOOP_DEV" 2>/dev/null
    fi
    rmdir "$MNT_DIR" 2>/dev/null
}
trap cleanup EXIT

# -----------------------------------------------------------
# Preflight
# -----------------------------------------------------------
for tool in dd mkfs.ext4 losetup mount umount; do
    command -v "$tool" >/dev/null 2>&1 || die "Required tool not found: $tool"
done

if [ "$(id -u)" -ne 0 ]; then
    # Check if we can sudo without a password, or remind the user
    if ! sudo -n true 2>/dev/null; then
        warn "This script requires sudo for loop-mount operations."
    fi
fi

# -----------------------------------------------------------
# Step 1 — Create the disk image
# -----------------------------------------------------------
mkdir -p "$OUTPUT_DIR"

info "Creating ${ROOTFS_SIZE_MB}MB ext4 disk image..."
dd if=/dev/zero of="$ROOTFS_IMG" bs=1M count="$ROOTFS_SIZE_MB" status=progress 2>&1

info "Formatting as ext4..."
mkfs.ext4 -F -L aios-root "$ROOTFS_IMG"

# -----------------------------------------------------------
# Step 2 — Mount via loop device
# -----------------------------------------------------------
info "Mounting disk image..."
LOOP_DEV="$(sudo losetup --find --show "$ROOTFS_IMG")"
sudo mount "$LOOP_DEV" "$MNT_DIR"

# -----------------------------------------------------------
# Step 3 — Create directory structure
# -----------------------------------------------------------
info "Creating filesystem hierarchy..."

sudo mkdir -p "${MNT_DIR}"/{bin,sbin,lib,lib64}
sudo mkdir -p "${MNT_DIR}"/usr/{bin,sbin,lib,lib64,share,libexec}
sudo mkdir -p "${MNT_DIR}"/etc/{aios/{agents/prompts,tools,models,security},network,init.d}
sudo mkdir -p "${MNT_DIR}"/var/{lib/aios/{models,memory,vectors,cache,ledger},log/aios,run/aios,tmp}
sudo mkdir -p "${MNT_DIR}"/run/aios/agents
sudo mkdir -p "${MNT_DIR}"/{proc,sys,dev,tmp,home/workspaces,root,opt,mnt,media,srv}
sudo mkdir -p "${MNT_DIR}"/boot

# Set permissions
sudo chmod 1777 "${MNT_DIR}/tmp"
sudo chmod 0700 "${MNT_DIR}/root"
sudo chmod 0755 "${MNT_DIR}/var/tmp"

# -----------------------------------------------------------
# Step 4 — Install BusyBox
# -----------------------------------------------------------
info "Installing BusyBox..."

mkdir -p build/cache
if [ ! -f "$BUSYBOX_BIN" ]; then
    info "Downloading BusyBox static binary..."
    wget -q --show-progress -O "$BUSYBOX_BIN" "$BUSYBOX_URL"
    chmod +x "$BUSYBOX_BIN"
fi

sudo cp "$BUSYBOX_BIN" "${MNT_DIR}/bin/busybox"
sudo chmod 755 "${MNT_DIR}/bin/busybox"

# Install all BusyBox applets as symlinks
sudo chroot "${MNT_DIR}" /bin/busybox --install -s /bin 2>/dev/null || {
    # If chroot fails (cross-arch), create essential symlinks manually
    warn "chroot failed (likely cross-arch build), creating symlinks manually..."
    for applet in sh ash ls cat cp mv rm mkdir rmdir mount umount \
                  ln chmod chown grep sed awk tr cut head tail sort \
                  uniq wc date hostname dmesg ps kill sleep \
                  ifconfig route ping ip netstat wget tar gzip gunzip \
                  vi less more find xargs du df free top reboot \
                  poweroff halt init syslogd klogd; do
        sudo ln -sf busybox "${MNT_DIR}/bin/${applet}"
    done
}

# -----------------------------------------------------------
# Step 5 — Install Rust binaries
# -----------------------------------------------------------
info "Installing aiOS binaries..."

MISSING_BINS=0
for bin_name in "${RUST_BINARIES[@]}"; do
    bin_path="${RUST_TARGET_DIR}/${bin_name}"
    if [ -f "$bin_path" ]; then
        sudo cp "$bin_path" "${MNT_DIR}/usr/sbin/${bin_name}"
        sudo chmod 755 "${MNT_DIR}/usr/sbin/${bin_name}"
        info "  Installed: ${bin_name} ($(du -h "$bin_path" | cut -f1))"
    else
        warn "  Binary not found: ${bin_path} — skipping (build Rust workspace first)"
        MISSING_BINS=$((MISSING_BINS + 1))
    fi
done

if [ "$MISSING_BINS" -gt 0 ]; then
    warn "${MISSING_BINS} Rust binaries not found. Build with:"
    warn "  cargo build --workspace --release --target x86_64-unknown-linux-musl"
fi

# -----------------------------------------------------------
# Step 6 — Install llama-server
# -----------------------------------------------------------
if [ -f "${OUTPUT_DIR}/bin/llama-server" ]; then
    info "Installing llama-server..."
    sudo cp "${OUTPUT_DIR}/bin/llama-server" "${MNT_DIR}/usr/bin/llama-server"
    sudo chmod 755 "${MNT_DIR}/usr/bin/llama-server"
else
    warn "llama-server not found at ${OUTPUT_DIR}/bin/llama-server — skipping"
fi

# -----------------------------------------------------------
# Step 7 — Install kernel modules
# -----------------------------------------------------------
if [ -d "${OUTPUT_DIR}/modules/lib/modules" ]; then
    info "Installing kernel modules..."
    sudo cp -a "${OUTPUT_DIR}/modules/lib/modules" "${MNT_DIR}/lib/"
else
    warn "Kernel modules not found at ${OUTPUT_DIR}/modules/ — skipping"
fi

# -----------------------------------------------------------
# Step 8 — Install models (if available)
# -----------------------------------------------------------
if [ -d "${OUTPUT_DIR}/models" ]; then
    info "Copying AI models..."
    for model_file in "${OUTPUT_DIR}/models/"*.gguf; do
        [ -f "$model_file" ] || continue
        model_name="$(basename "$model_file")"
        sudo cp "$model_file" "${MNT_DIR}/var/lib/aios/models/${model_name}"
        info "  Model: ${model_name} ($(du -h "$model_file" | cut -f1))"
    done
else
    info "No models directory found, skipping model installation."
fi

# -----------------------------------------------------------
# Step 9 — Create system configuration files
# -----------------------------------------------------------
info "Writing system configuration files..."

# /etc/aios/config.toml — master configuration
sudo tee "${MNT_DIR}/etc/aios/config.toml" > /dev/null << 'TOMLCONFIG'
# ============================================================
# aiOS System Configuration
# ============================================================

[system]
hostname = "aios"
log_level = "info"
log_file = "/var/log/aios/system.log"
autonomy_level = "supervised"
# autonomy_level options:
#   full       — AI operates completely autonomously
#   supervised — AI operates but logs decisions for human review
#   manual     — AI only acts on explicit human goals

[boot]
init_timeout_seconds = 300
debug_shell = true
clean_shutdown_flag = "/var/lib/aios/clean_shutdown"

[models]
runtime = "llama-cpp"
model_dir = "/var/lib/aios/models"
llama_server_binary = "/usr/bin/llama-server"

[models.operational]
file = "tinyllama-1.1b-chat.Q4_K_M.gguf"
always_loaded = true
context_length = 2048
threads = 2
gpu_layers = 0
max_tokens = 512
temperature = 0.1

[models.tactical]
file = "mistral-7b-instruct.Q4_K_M.gguf"
always_loaded = false
load_on_demand = true
context_length = 4096
threads = 4
gpu_layers = 0
max_tokens = 2048
temperature = 0.3
unload_after_idle_minutes = 5

[api]
enabled = true

[api.claude]
enabled = true
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-5-20250929"
max_tokens = 4096
temperature = 0.3
monthly_budget_usd = 100.0
rate_limit_rpm = 50
timeout_seconds = 30

[api.openai]
enabled = true
base_url = "https://api.openai.com"
model = "gpt-4o"
max_tokens = 4096
temperature = 0.3
monthly_budget_usd = 50.0
rate_limit_rpm = 50
timeout_seconds = 30
fallback_only = true

[api.cache]
enabled = true
max_entries = 1000
default_ttl_hours = 1

[memory]
operational_max_entries = 10000
working_db = "/var/lib/aios/memory/working.db"
longterm_db = "/var/lib/aios/memory/longterm.db"
vector_db_dir = "/var/lib/aios/vectors"
working_retention_days = 30
context_max_tokens = 4000

[security]
capability_mode = "strict"
audit_all_tool_calls = true
audit_db = "/var/lib/aios/ledger/audit.db"
sandbox_agents = true
secrets_file = "/etc/aios/secrets.enc"

[networking]
management_port = 9090
management_tls = false
dhcp_timeout_seconds = 30
dns_servers = ["1.1.1.1", "8.8.8.8"]
firewall_default_policy = "deny"
allow_outbound_https = true

[agents]
config_dir = "/etc/aios/agents"
prompts_dir = "/etc/aios/agents/prompts"
max_instances_per_type = 3
heartbeat_timeout_seconds = 15
max_restart_attempts = 5

[monitoring]
health_check_interval_seconds = 30
metric_collection_interval_seconds = 10
log_rotation_max_size_mb = 100
log_rotation_keep_files = 10
TOMLCONFIG

# /etc/hostname
echo "aios" | sudo tee "${MNT_DIR}/etc/hostname" > /dev/null

# /etc/hosts
sudo tee "${MNT_DIR}/etc/hosts" > /dev/null << 'EOF'
127.0.0.1       localhost
127.0.1.1       aios
::1             localhost ip6-localhost ip6-loopback
ff02::1         ip6-allnodes
ff02::2         ip6-allrouters
EOF

# /etc/resolv.conf
sudo tee "${MNT_DIR}/etc/resolv.conf" > /dev/null << 'EOF'
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF

# /etc/passwd
sudo tee "${MNT_DIR}/etc/passwd" > /dev/null << 'EOF'
root:x:0:0:root:/root:/bin/sh
daemon:x:1:1:daemon:/usr/sbin:/bin/false
nobody:x:65534:65534:nobody:/nonexistent:/bin/false
aios:x:1000:1000:aiOS Service Account:/var/lib/aios:/bin/false
EOF

# /etc/group
sudo tee "${MNT_DIR}/etc/group" > /dev/null << 'EOF'
root:x:0:
daemon:x:1:
nogroup:x:65534:
aios:x:1000:
EOF

# /etc/shadow (root has no password — serial console only; set one in production)
sudo tee "${MNT_DIR}/etc/shadow" > /dev/null << 'EOF'
root::0:0:99999:7:::
daemon:*:0:0:99999:7:::
nobody:*:0:0:99999:7:::
aios:*:0:0:99999:7:::
EOF
sudo chmod 640 "${MNT_DIR}/etc/shadow"

# /etc/fstab
sudo tee "${MNT_DIR}/etc/fstab" > /dev/null << 'EOF'
# <device>      <mount>   <type>     <options>             <dump> <pass>
/dev/vda        /         ext4       defaults,noatime      0      1
proc            /proc     proc       defaults              0      0
sysfs           /sys      sysfs      defaults              0      0
devtmpfs        /dev      devtmpfs   defaults              0      0
tmpfs           /tmp      tmpfs      defaults,nosuid,nodev 0      0
tmpfs           /run      tmpfs      defaults,nosuid,nodev 0      0
EOF

# /etc/os-release
sudo tee "${MNT_DIR}/etc/os-release" > /dev/null << 'EOF'
NAME="aiOS"
VERSION="0.1.0"
ID=aios
ID_LIKE=linux
VERSION_ID=0.1.0
PRETTY_NAME="aiOS 0.1.0 — AI-Native Operating System"
HOME_URL="https://github.com/mohamehr/aiOS"
EOF

# /etc/profile
sudo tee "${MNT_DIR}/etc/profile" > /dev/null << 'PROFILE'
export PATH="/usr/sbin:/usr/bin:/sbin:/bin"
export HOME="${HOME:-/root}"
export TERM="${TERM:-linux}"
export PS1='aios:\w\$ '

alias ll='ls -la'
alias la='ls -A'
PROFILE

# -----------------------------------------------------------
# Step 10 — Create service directory structure
# -----------------------------------------------------------
info "Creating service directories..."

# Inspired by systemd unit layout but simplified for aiOS
for svc in aios-init aios-orchestrator aios-tools aios-memory aios-api-gateway llama-server; do
    sudo mkdir -p "${MNT_DIR}/etc/aios/services/${svc}"
    sudo tee "${MNT_DIR}/etc/aios/services/${svc}/service.toml" > /dev/null << SVCEOF
[service]
name = "${svc}"
binary = "/usr/sbin/${svc}"
restart_policy = "always"
restart_delay_seconds = 2
max_restart_attempts = 5
SVCEOF
done

# Fix llama-server binary path
sudo sed -i 's|/usr/sbin/llama-server|/usr/bin/llama-server|' \
    "${MNT_DIR}/etc/aios/services/llama-server/service.toml" 2>/dev/null || true

# -----------------------------------------------------------
# Step 11 — Unmount
# -----------------------------------------------------------
info "Unmounting and finalizing..."
sudo umount "$MNT_DIR"
sudo losetup -d "$LOOP_DEV"
LOOP_DEV=""

# -----------------------------------------------------------
# Summary
# -----------------------------------------------------------
IMG_SIZE="$(du -h "$ROOTFS_IMG" | cut -f1)"

echo ""
ok "============================================"
ok " Root filesystem build complete"
ok "============================================"
ok " Image:  ${ROOTFS_IMG}  (${IMG_SIZE})"
ok "============================================"
