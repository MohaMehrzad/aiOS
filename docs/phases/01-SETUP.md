# Phase 1: Development Environment Setup

## Goal
Set up a complete development environment with all toolchains, emulators, and dependencies needed to build aiOS.

## Prerequisites
- A machine running Linux (Ubuntu 22.04+ recommended) or macOS 13+
- At least 32GB RAM, 256GB free disk space
- Internet connection
- Claude API key (for Claude Code and strategic layer development)

---

## Step-by-Step

### Step 1.1: Install Base Dependencies

**Claude Code prompt**: "Install all base build dependencies for cross-compiling a Linux kernel and Rust binaries"

```bash
# Ubuntu/Debian
sudo apt update && sudo apt install -y \
    build-essential \
    gcc \
    g++ \
    make \
    cmake \
    ninja-build \
    flex \
    bison \
    libssl-dev \
    libelf-dev \
    bc \
    cpio \
    wget \
    curl \
    git \
    python3 \
    python3-pip \
    python3-venv \
    qemu-system-x86 \
    qemu-utils \
    libguestfs-tools \
    debootstrap \
    parted \
    dosfstools \
    grub-pc-bin \
    grub-efi-amd64-bin \
    xorriso \
    mtools \
    protobuf-compiler \
    libprotobuf-dev

# macOS (using Homebrew) — for development only, target is Linux
brew install \
    cmake \
    ninja \
    protobuf \
    qemu \
    wget \
    python@3.12
```

### Step 1.2: Install Rust Toolchain

**Claude Code prompt**: "Set up Rust toolchain with stable and nightly channels, plus cross-compilation target"

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

# Install stable toolchain
rustup default stable
rustup component add clippy rustfmt

# Install target for cross-compilation (if developing on non-Linux)
rustup target add x86_64-unknown-linux-gnu

# Install useful cargo tools
cargo install cargo-watch cargo-expand
```

### Step 1.3: Install Python Environment

**Claude Code prompt**: "Set up Python virtual environment with AI and gRPC dependencies"

```bash
# Create project venv
cd /path/to/aiOS
python3 -m venv .venv
source .venv/bin/activate

# Install Python dependencies
pip install --upgrade pip
pip install \
    grpcio \
    grpcio-tools \
    protobuf \
    anthropic \
    openai \
    chromadb \
    pydantic \
    aiosqlite \
    httpx \
    asyncio \
    pytest \
    pytest-asyncio \
    ruff
```

### Step 1.4: Set Up QEMU Testing

**Claude Code prompt**: "Create QEMU launch scripts for testing aiOS kernel and rootfs"

Create the test infrastructure:

```bash
mkdir -p build/qemu

# Create a blank disk image for rootfs testing (2GB)
qemu-img create -f raw build/qemu/rootfs.img 2G

# Create QEMU launch script
cat > build/run-qemu.sh << 'SCRIPT'
#!/bin/bash
# Launch aiOS in QEMU for testing
KERNEL=${1:-"build/output/vmlinuz"}
ROOTFS=${2:-"build/qemu/rootfs.img"}
MEMORY=${3:-"4G"}
CPUS=${4:-"4"}

qemu-system-x86_64 \
    -kernel "$KERNEL" \
    -drive file="$ROOTFS",format=raw,if=virtio \
    -append "root=/dev/vda1 console=ttyS0 loglevel=3 init=/usr/sbin/aios-init" \
    -nographic \
    -m "$MEMORY" \
    -smp "$CPUS" \
    -enable-kvm \
    -netdev user,id=net0,hostfwd=tcp::9090-:9090,hostfwd=tcp::2222-:22 \
    -device virtio-net-pci,netdev=net0 \
    -serial mon:stdio
SCRIPT
chmod +x build/run-qemu.sh
```

### Step 1.5: Initialize Project Structure

**Claude Code prompt**: "Create the full aiOS project directory structure with Cargo workspace and Python package layout"

```
aiOS/
├── Cargo.toml               # Workspace root
├── kernel/
│   ├── configs/
│   │   └── aios-kernel.config
│   └── scripts/
│       ├── build-kernel.sh
│       └── build-initramfs.sh
├── initd/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── agent-core/
│   ├── Cargo.toml            # Rust orchestrator
│   ├── src/
│   │   └── main.rs
│   ├── proto/                 # gRPC definitions
│   │   ├── orchestrator.proto
│   │   ├── agent.proto
│   │   ├── tools.proto
│   │   └── memory.proto
│   └── python/                # Python agent runtime
│       ├── pyproject.toml
│       ├── aios_agent/
│       │   ├── __init__.py
│       │   ├── base.py
│       │   ├── orchestrator_client.py
│       │   └── agents/
│       │       ├── __init__.py
│       │       ├── system.py
│       │       ├── network.py
│       │       ├── security.py
│       │       ├── monitor.py
│       │       ├── package.py
│       │       ├── storage.py
│       │       ├── task.py
│       │       └── dev.py
│       └── tests/
├── tools/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── registry.rs
│       ├── fs/
│       ├── process/
│       ├── service/
│       ├── net/
│       ├── firewall/
│       ├── pkg/
│       ├── sec/
│       ├── monitor/
│       └── hw/
├── memory/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── operational.rs
│       ├── working.rs
│       ├── longterm.rs
│       └── knowledge.rs
├── api-gateway/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── claude.rs
│       ├── openai.rs
│       ├── router.rs
│       └── budget.rs
├── build/
│   ├── run-qemu.sh
│   ├── build-rootfs.sh
│   ├── build-iso.sh
│   └── Buildroot/
├── tests/
│   ├── integration/
│   │   ├── run.sh
│   │   └── test_boot.sh
│   └── e2e/
│       └── test_basic_goal.py
└── iso/                       # Build output
```

### Step 1.6: Create Cargo Workspace

```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "2"
members = [
    "initd",
    "agent-core",
    "tools",
    "memory",
    "api-gateway",
]

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
tonic = "0.12"
prost = "0.13"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = "0.3"
rusqlite = { version = "0.32", features = ["bundled"] }
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
```

### Step 1.7: Verify Everything Works

**Claude Code prompt**: "Verify the development environment: compile an empty Rust binary, run QEMU with a test kernel, and test Python imports"

```bash
# Test Rust compilation
cd /path/to/aiOS
cargo check --workspace

# Test Python environment
source .venv/bin/activate
python -c "import anthropic, chromadb, grpc; print('Python OK')"

# Test QEMU (with a dummy kernel — will fail to boot, but proves QEMU works)
qemu-system-x86_64 -nographic -m 256M -no-reboot -serial mon:stdio &
sleep 2 && kill %1
echo "QEMU OK"
```

---

## Deliverables Checklist

- [ ] All base build dependencies installed
- [ ] Rust toolchain (stable) with clippy and rustfmt
- [ ] Python 3.12+ with venv and all dependencies
- [ ] QEMU installed and working
- [ ] Project directory structure created
- [ ] Cargo workspace compiles (empty crates)
- [ ] Python package imports succeed
- [ ] `build/run-qemu.sh` script exists and is executable
- [ ] Git repository initialized with `.gitignore`

---

## Next Phase
Once all checks pass → [Phase 2: Kernel Build](./02-KERNEL.md)
