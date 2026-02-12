# aiOS — Claude Code Project Instructions

## Project Overview
aiOS is a custom AI-native Linux distribution where AI agents replace traditional system services. The AI IS the operating system — it controls init, scheduling, networking, packages, security, and all system operations autonomously.

## Tech Stack
- **Kernel**: Custom-compiled Linux 6.x (minimal config)
- **Core Language**: Rust (kernel-adjacent daemons, init, agent framework)
- **Agent Logic**: Python 3.12+ (AI orchestration, tool implementations)
- **Local AI Runtime**: llama.cpp / GGML (small models for routine ops)
- **External AI APIs**: Claude API (primary reasoning), OpenAI API (fallback)
- **IPC**: gRPC + Unix domain sockets
- **Memory/DB**: SQLite (structured), ChromaDB (vector/semantic)
- **Build System**: Custom shell scripts (kernel, rootfs, ISO assembly)
- **Containers**: Podman (rootless, for sandboxed tasks)

## Project Structure
```
aiOS/
├── CLAUDE.md                  # This file — Claude Code instructions
├── Cargo.toml                 # Workspace root (6 Rust crates)
├── docs/                      # Full documentation suite (33 files)
│   ├── README.md              # Doc index and navigation
│   ├── VISION.md              # Project vision and philosophy
│   ├── HARDWARE.md            # Hardware requirements
│   ├── ROADMAP.md             # Phase-by-phase roadmap
│   ├── architecture/          # Architecture deep-dives
│   └── phases/                # Step-by-step build phases (01-12)
├── kernel/                    # Kernel configs (aios-kernel.config)
├── initd/                     # AI init daemon (Rust, PID 1)
├── agent-core/                # Orchestrator + Agent framework
│   ├── src/                   # Rust: goal engine, task planner, agent router
│   ├── proto/                 # 7 gRPC proto definitions
│   └── python/                # Python agent package (aios_agent)
│       └── aios_agent/agents/ # 8 agent types (system, task, network, security, etc.)
├── runtime/                   # AI Runtime — llama.cpp wrapper (Rust gRPC)
├── tools/                     # Tool registry + 40+ system tools (Rust)
│   └── src/                   # fs/, process/, service/, net/, firewall/,
│                              # pkg/, sec/, monitor/, hw/ namespaces
├── memory/                    # Three-tier memory system (Rust gRPC)
├── api-gateway/               # Claude/OpenAI API gateway (Rust gRPC)
├── config/                    # Default system configuration
├── rootfs/                    # Root filesystem overlay
│   └── etc/                   # AppArmor profiles, agent configs, security policy
├── scripts/                   # Build scripts (kernel, rootfs, ISO, QEMU)
├── tests/                     # Integration and E2E tests
├── build/                     # Build output directory
└── iso/                       # Final ISO output
```

## Coding Standards

### Rust Code
- Edition 2021, stable toolchain
- Use `anyhow` for error handling in binaries, `thiserror` in libraries
- All public APIs must have doc comments
- Run `cargo clippy` before committing
- Format with `rustfmt`
- No `unwrap()` in production code — use `?` or explicit error handling

### Python Code
- Python 3.12+
- Type hints on all function signatures
- Use `asyncio` for all I/O operations
- Format with `ruff`
- Use `pydantic` for data validation
- No bare `except:` — always catch specific exceptions

### Commit Messages
- Format: `[component] short description`
- Examples: `[kernel] strip unnecessary drivers`, `[agent-core] add task decomposition`
- Components: kernel, initd, agent-core, tools, memory, networking, security, api-gateway, pkg-manager, build, tests, docs

## Claude Code Workflow

### Starting a New Phase
1. Read the phase doc in `docs/phases/XX-NAME.md`
2. Read the relevant architecture doc in `docs/architecture/`
3. Create the directory structure defined in the phase doc
4. Implement step by step, testing each component
5. Run the phase-specific tests before moving on

### Key Conventions
- Every system operation must be exposed as a Tool in the tool registry
- All agent-to-agent communication goes through gRPC
- Local models handle: file monitoring, log analysis, simple decisions, routine system ops
- Claude/GPT API handles: planning, complex reasoning, code generation, security analysis
- Every action is logged to the audit ledger
- The AI must be able to explain every decision it makes

### Testing
- Unit tests: `cargo test` (Rust), `pytest` (Python)
- Integration tests: Docker-based system tests in `tests/`
- VM tests: QEMU-based boot and operation tests
- See `docs/guides/TESTING.md` for full strategy

### Build Commands
```bash
# Build kernel
cd kernel && make -j$(nproc)

# Build Rust components
cargo build --workspace --release

# Build Python components
cd agent-core && pip install -e .

# Build full ISO
cd build && ./build-iso.sh

# Run in QEMU
cd build && ./run-qemu.sh

# Run tests
cargo test --workspace
pytest tests/
./tests/integration/run.sh
```
