# Packages & Dependencies

## Rust Crates (Cargo)

### Workspace Dependencies (shared across all crates)

| Crate | Version | Purpose |
|---|---|---|
| `tokio` | 1.x | Async runtime (full features) |
| `tonic` | 0.12 | gRPC framework |
| `prost` | 0.13 | Protocol Buffers |
| `serde` | 1.x | Serialization/deserialization |
| `serde_json` | 1.x | JSON handling |
| `anyhow` | 1.x | Error handling (binaries) |
| `thiserror` | 2.x | Error handling (libraries) |
| `tracing` | 0.1 | Structured logging |
| `tracing-subscriber` | 0.3 | Log output formatting |
| `rusqlite` | 0.32 | SQLite database (bundled) |
| `uuid` | 1.x | UUID generation |
| `chrono` | 0.4 | Date/time handling |
| `axum` | 0.7 | HTTP framework (management console) |
| `tower-http` | 0.5 | HTTP middleware (static files, CORS) |

### Per-Crate Dependencies

#### `aios-init` (initd/)
| Crate | Purpose |
|---|---|
| `nix` | Unix system calls (mount, signals, processes) |
| `toml` | Config file parsing |
| `log` | Logging facade |

**Build note**: Must compile with `--target x86_64-unknown-linux-musl` for static linking.

#### `aios-orchestrator` (agent-core/)
| Crate | Purpose |
|---|---|
| `tonic` | gRPC server and client |
| `prost` | Protobuf code generation |
| `petgraph` | Task DAG (dependency graph) |
| `priority-queue` | Goal priority queue |
| `dashmap` | Concurrent HashMap for agent registry |

#### `aios-tools` (tools/)
| Crate | Purpose |
|---|---|
| `tonic` | gRPC server |
| `jsonschema` | JSON schema validation |
| `sha2` | SHA-256 for audit hashing |
| `walkdir` | Directory traversal (fs.search) |
| `sysinfo` | System information (monitor tools) |
| `nix` | Process management (process tools) |

#### `aios-memory` (memory/)
| Crate | Purpose |
|---|---|
| `rusqlite` | Working memory + long-term structured storage |
| `tonic` | gRPC server |

#### `aios-api-gateway` (api-gateway/)
| Crate | Purpose |
|---|---|
| `reqwest` | HTTP client for API calls |
| `tonic` | gRPC server |
| `lru` | LRU cache for response caching |

---

## Python Packages (pip)

### Core
| Package | Version | Purpose |
|---|---|---|
| `grpcio` | 1.60+ | gRPC client/server |
| `grpcio-tools` | 1.60+ | Protobuf code generation |
| `protobuf` | 4.x | Protocol Buffers runtime |
| `pydantic` | 2.x | Data validation and settings |
| `aiosqlite` | 0.20+ | Async SQLite |
| `httpx` | 0.27+ | Async HTTP client |

### AI
| Package | Version | Purpose |
|---|---|---|
| `anthropic` | 0.40+ | Claude API client |
| `openai` | 1.50+ | OpenAI API client |
| `chromadb` | 0.5+ | Vector database for long-term memory |

### Development & Testing
| Package | Version | Purpose |
|---|---|---|
| `pytest` | 8.x | Testing framework |
| `pytest-asyncio` | 0.24+ | Async test support |
| `ruff` | 0.5+ | Linter and formatter |
| `mypy` | 1.10+ | Type checking |

---

## System Packages (in rootfs)

### Base System
| Package | Source | Purpose |
|---|---|---|
| BusyBox | busybox.net (static binary) | Core utilities (sh, ls, cp, etc.) |
| musl libc | musl.libc.org | C library (for Rust static binaries) |

### AI Runtime
| Package | Source | Purpose |
|---|---|---|
| llama.cpp (llama-server) | github.com/ggerganov/llama.cpp | Local model inference server |

### Networking
| Package | Source | Purpose |
|---|---|---|
| unbound | nlnetlabs.nl/unbound | DNS resolver |
| nftables (nft) | netfilter.org | Firewall management |
| WireGuard tools | wireguard.com | VPN client/server |
| iproute2 (ip) | kernel.org | Network interface management |
| curl | curl.se | HTTP client (for testing/debugging) |

### Security
| Package | Source | Purpose |
|---|---|---|
| AppArmor (apparmor_parser) | apparmor.net | Application sandboxing |
| OpenSSH (sshd) | openssh.com | Emergency SSH access |

### Containers
| Package | Source | Purpose |
|---|---|---|
| Podman | podman.io | Rootless containers for sandboxed tasks |
| crun | github.com/containers/crun | OCI container runtime |

### Package Management
| Package | Source | Purpose |
|---|---|---|
| apk-tools | alpinelinux.org | Alpine package manager |

### Boot
| Package | Source | Purpose |
|---|---|---|
| GRUB 2 | gnu.org/software/grub | Bootloader |

---

## AI Models (GGUF files)

| Model | Size | Layer | Always Loaded | Purpose |
|---|---|---|---|---|
| TinyLlama 1.1B Chat Q4_K_M | ~700 MB | Operational | Yes | Quick decisions, classification, log parsing |
| Phi-3 Mini 3.8B Q4_K_M | ~2.3 GB | Tactical | On demand | Task routing, NLU, simple reasoning |
| Mistral 7B Instruct Q4_K_M | ~4.4 GB | Tactical | On demand | Complex local reasoning, config generation |

### Optional (Tier 3 hardware)
| Model | Size | Layer | Purpose |
|---|---|---|---|
| Llama 3.1 13B Q4_K_M | ~7.5 GB | Tactical+ | Near-strategic quality locally |
| Llama 3.1 70B Q4_K_M | ~40 GB | Strategic (local) | Full strategic layer without API |

---

## Build Dependencies (dev machine only)

| Tool | Version | Purpose |
|---|---|---|
| gcc/g++ | 12+ | Kernel and C compilation |
| make | 4.x | Build system |
| cmake | 3.22+ | llama.cpp build |
| ninja | 1.11+ | Fast build system |
| flex | 2.6+ | Kernel build dependency |
| bison | 3.8+ | Kernel build dependency |
| libssl-dev | 3.x | Kernel crypto |
| libelf-dev | 0.190+ | Kernel module support |
| bc | 1.07+ | Kernel build calculations |
| cpio | 2.13+ | Initramfs creation |
| protoc | 25+ | Protocol Buffers compiler |
| rustup + cargo | stable | Rust toolchain |
| python3 | 3.12+ | Python runtime |
| qemu-system-x86 | 8.x+ | VM testing |
| grub-mkrescue | 2.x | ISO creation |
| xorriso | 1.5+ | ISO creation |
| debootstrap | 1.0+ | Root filesystem bootstrap (optional) |
| parted | 3.x | Disk partitioning |
