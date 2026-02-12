# aiOS Documentation Index

> **aiOS** — A custom Linux distribution where AI agents ARE the operating system.

This documentation suite is the complete blueprint for building aiOS from scratch. Every doc is written to be consumed by both humans and Claude Code for agentic development.

---

## How to Navigate

| If you want to... | Read this |
|---|---|
| Understand the vision | [VISION.md](./VISION.md) |
| See the full architecture | [architecture/SYSTEM.md](./architecture/SYSTEM.md) |
| Know what hardware you need | [HARDWARE.md](./HARDWARE.md) |
| See all packages/dependencies | [PACKAGES.md](./PACKAGES.md) |
| Follow the build roadmap | [ROADMAP.md](./ROADMAP.md) |
| Start building (Phase 1) | [phases/01-SETUP.md](./phases/01-SETUP.md) |
| Use Claude Code on this project | [guides/CLAUDE-WORKFLOW.md](./guides/CLAUDE-WORKFLOW.md) |
| Understand the test strategy | [guides/TESTING.md](./guides/TESTING.md) |

---

## Architecture Deep-Dives

| Document | Covers |
|---|---|
| [SYSTEM.md](./architecture/SYSTEM.md) | Full system architecture, layer diagram, data flow |
| [KERNEL.md](./architecture/KERNEL.md) | Kernel configuration, module selection, boot process |
| [AGENT-FRAMEWORK.md](./architecture/AGENT-FRAMEWORK.md) | Agent mesh, orchestrator, task routing, mini-LLM swarm |
| [TOOL-REGISTRY.md](./architecture/TOOL-REGISTRY.md) | Tool system, API design, how AI calls system operations |
| [MEMORY-SYSTEM.md](./architecture/MEMORY-SYSTEM.md) | Persistent memory, vector DB, knowledge graph, context management |
| [SECURITY.md](./architecture/SECURITY.md) | Capability-based security, sandboxing, audit, threat model |
| [NETWORKING.md](./architecture/NETWORKING.md) | AI-controlled networking, firewall, DNS, service mesh |
| [MANAGEMENT-CONSOLE.md](./architecture/MANAGEMENT-CONSOLE.md) | REST API, dashboard, emergency controls, authentication |
| [ERROR-RECOVERY.md](./architecture/ERROR-RECOVERY.md) | Watchdog, service supervision, crash recovery, degraded modes |
| [AGENT-CONFIGS.md](./architecture/AGENT-CONFIGS.md) | Per-agent TOML config schema, capabilities, resource limits |
| [PYTHON-PACKAGING.md](./architecture/PYTHON-PACKAGING.md) | Python project structure, pyproject.toml, proto generation |
| [PROTO-DEFINITIONS.md](./architecture/PROTO-DEFINITIONS.md) | Complete gRPC proto files (tools.proto, memory.proto, etc.) |
| [CONFIG-REFERENCE.md](./architecture/CONFIG-REFERENCE.md) | Full config.toml schema, secrets format, loading order |

---

## Build Phases (Sequential)

Each phase builds on the previous one. Follow in order.
Also read [PHASE-TRANSITIONS.md](./phases/PHASE-TRANSITIONS.md) for how `aios-init` evolves between phases.

| Phase | Document | What You Build |
|---|---|---|
| 1 | [01-SETUP.md](./phases/01-SETUP.md) | Development environment, toolchains, QEMU |
| 2 | [02-KERNEL.md](./phases/02-KERNEL.md) | Custom minimal Linux kernel |
| 3 | [03-BASE-SYSTEM.md](./phases/03-BASE-SYSTEM.md) | Minimal userspace, BusyBox, filesystem layout |
| 4 | [04-AI-RUNTIME.md](./phases/04-AI-RUNTIME.md) | Local model runtime (llama.cpp, model management) |
| 5 | [05-AGENT-CORE.md](./phases/05-AGENT-CORE.md) | Core agent framework, orchestrator, agent mesh |
| 6 | [06-TOOLS.md](./phases/06-TOOLS.md) | Tool registry, system tool implementations |
| 7 | [07-MEMORY.md](./phases/07-MEMORY.md) | Memory system, vector DB, knowledge persistence |
| 8 | [08-NETWORKING.md](./phases/08-NETWORKING.md) | AI-controlled networking stack |
| 9 | [09-SECURITY.md](./phases/09-SECURITY.md) | Security subsystem, capabilities, sandboxing |
| 10 | [10-PACKAGES.md](./phases/10-PACKAGES.md) | AI-driven package manager |
| 11 | [11-API-GATEWAY.md](./phases/11-API-GATEWAY.md) | External API integration (Claude, OpenAI) |
| 12 | [12-DISTRO.md](./phases/12-DISTRO.md) | Final ISO build, distribution, deployment |

---

## For Claude Code

When working on this project with Claude Code:
1. Always read `CLAUDE.md` at project root first
2. Before starting a phase, read both the phase doc AND the relevant architecture doc
3. Follow the coding standards in `CLAUDE.md`
4. See [guides/CLAUDE-WORKFLOW.md](./guides/CLAUDE-WORKFLOW.md) for Claude-specific workflow
