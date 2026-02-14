# aiOS

[![CI](https://github.com/MohaMehrzad/aiOS/actions/workflows/ci.yml/badge.svg)](https://github.com/MohaMehrzad/aiOS/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Python](https://img.shields.io/badge/Python-3.12+-3776AB.svg)](https://www.python.org/)
[![GitHub Stars](https://img.shields.io/github/stars/MohaMehrzad/aiOS?style=social)](https://github.com/MohaMehrzad/aiOS)

**A custom Linux distribution where AI agents ARE the operating system.**

aiOS replaces traditional system services with autonomous AI agents. Instead of cron jobs, systemd units, and manual administration, aiOS uses a hierarchy of local and cloud AI models to manage init, scheduling, networking, packages, security, and all system operations autonomously.

> **Why aiOS?** Traditional Linux administration requires deep expertise and constant manual intervention. aiOS eliminates this by making AI the first-class citizen of the OS — every system operation is an AI decision, every service is an autonomous agent, and the system learns and adapts over time.

## Quick Start

```bash
# Clone the repo
git clone https://github.com/MohaMehrzad/aiOS.git && cd aiOS

# Build all Rust services
cargo build --workspace --release

# Run tests
cargo test --workspace

# Start the system (requires config)
./scripts/deploy-local.sh
```

## Architecture

```
              Management Console (HTTP :9090)
                        |
                   Orchestrator
              /    |    |    |    \
         Goal   Task  Agent  Result  Decision
        Engine  Planner Router Aggregator Logger
                        |
           +------------+------------+
           |            |            |
      AI Runtime   Tool Service  Memory Service
     (llama.cpp)   (fs/net/proc)  (3-tier store)
           |            |            |
      Local Models  System Ops   SQLite + Vector
           |
      API Gateway
     (Claude/OpenAI)
```

### Intelligence Hierarchy

| Level | Model | Use Case |
|-------|-------|----------|
| Reactive | Heuristics | Health checks, status, ping |
| Operational | TinyLlama 1.1B | File ops, log parsing, monitoring |
| Tactical | Mistral 7B | Multi-step tasks, service management |
| Strategic | Claude / OpenAI | Architecture, security analysis, planning |

## Crate Structure

| Crate | Binary | Description |
|-------|--------|-------------|
| `initd` | `aios-init` | PID 1 init daemon with service supervision |
| `agent-core` | `aios-orchestrator` | Goal engine, task planner, agent router, management console |
| `tools` | `aios-tools` | Tool registry with 12 system tools (fs, process, service, network, package, security, hardware, monitoring) |
| `memory` | `aios-memory` | Three-tier memory: working (goals/tasks), operational (events/metrics), long-term (knowledge/procedures) |
| `api-gateway` | `aios-api-gateway` | External API gateway with Claude (primary) and OpenAI (fallback), budget tracking, response caching |
| `runtime` | `aios-runtime` | Local AI model management via llama.cpp, GGUF model loading, inference engine |

## Python Agents

The `python/aios_agents/` package provides 8 specialized agent types:

| Agent | Role |
|-------|------|
| `base_agent` | Abstract base with gRPC registration and heartbeat |
| `system_agent` | System health monitoring and resource management |
| `network_agent` | DNS, firewall, and connectivity management |
| `security_agent` | File permission auditing and vulnerability scanning |
| `package_agent` | Package installation, updates, and dependency resolution |
| `monitor_agent` | Continuous metric collection and anomaly detection |
| `maintenance_agent` | Log rotation, temp cleanup, disk management |
| `learning_agent` | Pattern recognition and operational learning |

## Tech Stack

- **Core Language**: Rust (all system services)
- **Agent Logic**: Python 3.12+
- **Local AI**: llama.cpp / GGUF models
- **External AI**: Claude API (primary), OpenAI API (fallback)
- **IPC**: gRPC with Protocol Buffers
- **Storage**: SQLite (structured), vector embeddings (semantic search)
- **Build System**: Custom shell scripts for kernel, rootfs, and ISO assembly
- **Sandboxing**: Podman (rootless containers), AppArmor profiles

## Project Stats

| Metric | Value |
|--------|-------|
| Rust source files | 92 |
| Python source files | 24 |
| Shell scripts | 12 |
| Proto definitions | 7 |
| Rust lines of code | ~18,300 |
| Python lines of code | ~10,500 |
| Rust unit tests | 230 |
| Python unit tests | 303 |
| Total lines of code | ~28,800 |

## Building

### Prerequisites

- Rust toolchain (stable)
- Python 3.12+
- Protocol Buffers compiler (`protoc`)

### Build All Crates

```bash
cargo build --release
```

### Run Tests

```bash
# Rust tests
cargo test

# Python tests
cd python && python -m pytest tests/ -v
```

### Build Root Filesystem

```bash
# Requires root — builds the complete aiOS root filesystem
sudo scripts/build-rootfs.sh
```

### Build ISO Image

```bash
# Assemble bootable ISO from rootfs
scripts/build-iso.sh
```

## Configuration

aiOS uses a layered TOML configuration:

```
/etc/aios/config.toml      # Main system config
/etc/aios/agents/*.toml     # Per-agent configs
/etc/aios/secrets.toml      # API keys (600 permissions)
```

See [docs/architecture/CONFIG-REFERENCE.md](docs/architecture/CONFIG-REFERENCE.md) for the full schema.

## Documentation

Comprehensive documentation lives in [`docs/`](docs/README.md):

- [Vision](docs/VISION.md) -- Project goals and philosophy
- [System Architecture](docs/architecture/SYSTEM.md) -- Full system design
- [Build Roadmap](docs/ROADMAP.md) -- 12-phase build plan
- [Agent Framework](docs/architecture/AGENT-FRAMEWORK.md) -- Agent mesh and orchestration
- [Tool Registry](docs/architecture/TOOL-REGISTRY.md) -- Tool system and API design
- [Memory System](docs/architecture/MEMORY-SYSTEM.md) -- Three-tier memory architecture
- [Security](docs/architecture/SECURITY.md) -- Capability-based security model
- [Networking](docs/architecture/NETWORKING.md) -- AI-controlled networking stack

## gRPC Services

| Service | Port | Protocol |
|---------|------|----------|
| Orchestrator | 50051 | gRPC |
| AI Runtime | 50052 | gRPC |
| Tool Service | 50053 | gRPC |
| Memory Service | 50054 | gRPC |
| API Gateway | 50055 | gRPC |
| Management Console | 9090 | HTTP/REST |
| llama.cpp | 8080 | HTTP |

## Security Model

aiOS uses defense-in-depth with capability-based access control:

- **Tool permissions**: Each tool declares required capabilities; agents must possess matching capabilities
- **Sandboxing**: Podman rootless containers for untrusted operations
- **AppArmor**: Mandatory access control profiles for all services
- **Audit trail**: Hash-chained audit log for every tool execution
- **Budget controls**: Per-provider spending limits for external API calls
- **Reversible operations**: Pre-execution backups with rollback support

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

- [Open an Issue](https://github.com/MohaMehrzad/aiOS/issues/new/choose) to report bugs or request features
- [Start a Discussion](https://github.com/MohaMehrzad/aiOS/discussions) for questions and ideas
- Check the [Roadmap](docs/ROADMAP.md) to see what's planned

## License

This project is licensed under the MIT License — see [LICENSE](LICENSE) for details.
