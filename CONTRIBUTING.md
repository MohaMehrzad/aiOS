# Contributing to aiOS

Thanks for your interest in contributing to aiOS! This project is building an AI-native Linux distribution where autonomous AI agents replace traditional system services.

## Getting Started

1. **Fork** the repository
2. **Clone** your fork: `git clone https://github.com/YOUR_USERNAME/aiOS.git`
3. **Create a branch**: `git checkout -b my-feature`
4. **Make your changes**
5. **Test**: `cargo test --workspace && cd python && python -m pytest tests/ -v`
6. **Commit**: `git commit -m "[component] short description"`
7. **Push**: `git push origin my-feature`
8. **Open a Pull Request**

## Development Setup

### Prerequisites

- Rust toolchain (stable, edition 2021)
- Python 3.12+
- Protocol Buffers compiler (`protoc`)
- SQLite 3

### Build

```bash
# Build all Rust crates
cargo build --workspace

# Install Python package in dev mode
cd agent-core && pip install -e ".[dev]"
```

### Run Tests

```bash
# Rust
cargo test --workspace
cargo clippy --workspace -- -D warnings

# Python
cd python && python -m pytest tests/ -v
```

## Commit Message Format

Use the format: `[component] short description`

Components: `kernel`, `initd`, `agent-core`, `tools`, `memory`, `runtime`, `api-gateway`, `build`, `tests`, `docs`

Examples:
- `[agent-core] add task decomposition for multi-step goals`
- `[tools] implement firewall rule management`
- `[docs] update memory system architecture guide`

## Code Standards

### Rust
- Use `anyhow` for error handling in binaries, `thiserror` in libraries
- No `unwrap()` in production code — use `?` or explicit error handling
- All public APIs must have doc comments
- Run `cargo clippy` and `cargo fmt` before committing

### Python
- Type hints on all function signatures
- Use `asyncio` for all I/O operations
- No bare `except:` — always catch specific exceptions
- Format with `ruff`

## What to Contribute

- **Bug fixes** — Check the [issues](https://github.com/MohaMehrzad/aiOS/issues) for reported bugs
- **New tools** — The tool registry (`tools/src/`) is designed for extension
- **Agent types** — New Python agents in `agent-core/python/aios_agent/agents/`
- **Documentation** — Improvements to docs, examples, guides
- **Tests** — More test coverage is always welcome
- **Performance** — Profiling and optimization of hot paths

## Architecture Overview

Before diving in, read these docs:
- [System Architecture](docs/architecture/SYSTEM.md)
- [Agent Framework](docs/architecture/AGENT-FRAMEWORK.md)
- [Tool Registry](docs/architecture/TOOL-REGISTRY.md)

## Questions?

Open a [Discussion](https://github.com/MohaMehrzad/aiOS/discussions) or an [Issue](https://github.com/MohaMehrzad/aiOS/issues).
