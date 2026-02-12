# aiOS

**A custom Linux distribution where AI agents ARE the operating system.**

aiOS replaces traditional system services with autonomous AI agents. Instead of cron jobs, systemd units, and manual administration, aiOS uses a hierarchy of local and cloud AI models to manage init, scheduling, networking, packages, security, and all system operations autonomously.

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
