# aiOS Build Roadmap

## Overview

Building aiOS is a 12-phase process. Each phase produces a testable, bootable system with incrementally more AI capability.

**Total estimated LOC**: ~25,000-35,000 (Rust + Python)

---

## Phase Map

```
Phase 1: Dev Setup          ████░░░░░░░░░░░░░░░░  Week 1
Phase 2: Kernel             ████░░░░░░░░░░░░░░░░  Week 1-2
Phase 3: Base System        ████░░░░░░░░░░░░░░░░  Week 2-3
Phase 4: AI Runtime         ██████░░░░░░░░░░░░░░  Week 3-4
Phase 5: Agent Core         ██████████░░░░░░░░░░  Week 4-6
Phase 6: Tool Registry      ████████░░░░░░░░░░░░  Week 6-7
Phase 7: Memory System      ██████░░░░░░░░░░░░░░  Week 7-8
Phase 8: Networking         ██████░░░░░░░░░░░░░░  Week 8-9
Phase 9: Security           ██████░░░░░░░░░░░░░░  Week 9-10
Phase 10: Package Manager   ████░░░░░░░░░░░░░░░░  Week 10-11
Phase 11: API Gateway       ████░░░░░░░░░░░░░░░░  Week 11-12
Phase 12: Distro Packaging  ████░░░░░░░░░░░░░░░░  Week 12-13
```

---

## Phase Details

### Phase 1: Development Environment Setup
**Goal**: Fully working dev environment with cross-compilation and QEMU testing
**Output**: Can compile Rust for target, run QEMU VM, build initramfs
**Key Deliverable**: `./build/setup-dev.sh` script that sets up everything
**Dependencies**: None

### Phase 2: Custom Kernel Build
**Goal**: Minimal Linux kernel that boots to our init
**Output**: `vmlinuz` + `initramfs.img` that boots in QEMU in <5 seconds
**Key Deliverable**: Kernel config + build scripts
**Dependencies**: Phase 1

### Phase 3: Base System (Minimal Userspace)
**Goal**: Bootable system with BusyBox, filesystem layout, and our custom init stub
**Output**: A rootfs image that boots, mounts filesystems, and runs PID 1
**Key Deliverable**: Root filesystem with aios-init (skeleton)
**Dependencies**: Phase 2

### Phase 4: Local AI Runtime
**Goal**: llama.cpp running as a system service, loadable models, inference API
**Output**: Can send prompts to local models and get responses
**Key Deliverable**: `aios-runtime` daemon + model management
**Dependencies**: Phase 3

### Phase 5: Agent Core Framework
**Goal**: Orchestrator + agent mesh + task planning + intelligence routing
**Output**: Can decompose a goal into tasks and route to agents
**Key Deliverable**: `aios-orchestrator` + agent runtime + gRPC APIs
**Dependencies**: Phase 4

### Phase 6: Tool Registry & System Tools
**Goal**: Complete tool system with all core tools implemented
**Output**: Agents can perform file, process, and service operations through tools
**Key Deliverable**: `aios-tools` service + 30+ tool implementations
**Dependencies**: Phase 5

### Phase 7: Memory System
**Goal**: Three-tier memory with vector search
**Output**: Agents have persistent memory, context assembly works
**Key Deliverable**: `aios-memory` service + ChromaDB integration
**Dependencies**: Phase 5

### Phase 8: AI-Controlled Networking
**Goal**: Network Agent manages all networking autonomously
**Output**: System configures its own network, firewall, DNS
**Key Deliverable**: Network tools + Network Agent
**Dependencies**: Phase 6

### Phase 9: Security Subsystem
**Goal**: Capability-based security, sandboxing, audit, IDS
**Output**: Agents are sandboxed, all actions audited, intrusion detection active
**Key Deliverable**: Security Agent + capability system + audit ledger
**Dependencies**: Phase 6, Phase 7

### Phase 10: AI Package Manager
**Goal**: Package Agent can install, update, remove software autonomously
**Output**: Can install packages, resolve dependencies, track vulnerabilities
**Key Deliverable**: Package Agent + package tools
**Dependencies**: Phase 8, Phase 9

### Phase 11: External API Gateway
**Goal**: Claude and OpenAI integration with budget management and fallback
**Output**: Strategic layer operational, full intelligence hierarchy working
**Key Deliverable**: API gateway service + cost tracking
**Dependencies**: Phase 5

### Phase 12: Distribution Packaging
**Goal**: Build pipeline that produces bootable ISO/image
**Output**: Downloadable ISO that installs aiOS on bare metal or VM
**Key Deliverable**: `build-iso.sh` + installation scripts + docs
**Dependencies**: All previous phases

---

## Milestone Checkpoints

| Milestone | After Phase | What You Can Do |
|---|---|---|
| **First Boot** | 3 | System boots, init runs, serial console works |
| **First AI Response** | 4 | Send prompt to local model, get response |
| **First Autonomous Action** | 6 | AI decomposes a goal and executes tool calls |
| **Learning System** | 7 | AI remembers past actions and decisions |
| **Self-Managing Network** | 8 | AI configures its own networking |
| **Secure Operation** | 9 | Full audit trail, sandboxed agents |
| **Self-Updating** | 10 | AI can install and update its own packages |
| **Full Intelligence** | 11 | Local + API models working in hierarchy |
| **Distributable** | 12 | ISO that anyone can boot |

---

## Parallel Work Streams

Some phases can be developed in parallel:

```
Sequential: 1 → 2 → 3 → 4 → 5 ──┬── 6 ──┬── 8 ──┬── 10 → 12
                                   │       │       │
                                   ├── 7   ├── 9   │
                                   │       │       │
                                   └── 11  └───────┘
```

- Phase 6 (Tools) and Phase 7 (Memory) can be developed in parallel after Phase 5
- Phase 8 (Network) and Phase 9 (Security) can be developed in parallel after Phase 6
- Phase 11 (API Gateway) can start as early as Phase 5

---

## Claude Code Session Planning

Each phase maps to 1-3 Claude Code sessions:

| Phase | Sessions | Session Focus |
|---|---|---|
| 1 | 1 | Setup scripts, verify toolchain |
| 2 | 1 | Kernel config, build, QEMU test |
| 3 | 1-2 | Rootfs, init daemon skeleton |
| 4 | 2 | llama.cpp integration, model management |
| 5 | 3-4 | Orchestrator, agents, gRPC, routing |
| 6 | 2-3 | Tool service, 30+ tool implementations |
| 7 | 2 | SQLite schemas, ChromaDB, context assembly |
| 8 | 1-2 | Network tools, Network Agent logic |
| 9 | 2-3 | Capabilities, sandboxing, audit, IDS |
| 10 | 1-2 | Package tools, Package Agent |
| 11 | 1-2 | API client, budget tracking, fallback |
| 12 | 1-2 | Build scripts, ISO generation, installer |

**Total: ~20-28 Claude Code sessions**

Each session:
1. Read the phase doc + architecture doc
2. Implement the components
3. Write tests
4. Verify in QEMU
5. Commit
