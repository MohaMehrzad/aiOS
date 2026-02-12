# Claude Code Workflow Guide

## How to Use Claude Code to Build aiOS

This guide explains exactly how to use Claude Code (this tool) to build each component of aiOS efficiently.

---

## Session Structure

Each coding session should follow this pattern:

### 1. Context Loading (Start of Session)
```
You: "Read docs/phases/05-AGENT-CORE.md and docs/architecture/AGENT-FRAMEWORK.md"
```
Claude Code reads both docs, understands the requirements.

### 2. Incremental Implementation
Don't ask Claude to build an entire phase at once. Break it into components:
```
You: "Implement the Goal Engine in agent-core/src/goal_engine.rs"
You: "Now implement the Task Planner in agent-core/src/task_planner.rs"
You: "Now implement the Agent Router in agent-core/src/agent_router.rs"
```

### 3. Test After Each Component
```
You: "Write tests for goal_engine.rs and run them"
You: "Run cargo clippy on the workspace"
```

### 4. Integration at Phase End
```
You: "Boot the system in QEMU and verify the orchestrator starts"
```

---

## Recommended Prompts Per Phase

### Phase 1: Setup
```
"Set up the development environment following docs/phases/01-SETUP.md.
 Install all dependencies, create the project structure, and verify
 the Cargo workspace compiles."
```

### Phase 2: Kernel
```
"Following docs/phases/02-KERNEL.md:
 1. Download the Linux 6.8 kernel source
 2. Create the minimal kernel config from docs/architecture/KERNEL.md
 3. Build the kernel
 4. Create a minimal initramfs
 5. Test boot in QEMU"
```

### Phase 3: Base System
```
"Following docs/phases/03-BASE-SYSTEM.md:
 Implement aios-init in Rust (initd/src/main.rs). It should:
 - Mount proc, sys, dev, tmp, run
 - Read config from /etc/aios/config.toml
 - Detect hardware (CPU, RAM, GPU, storage, network)
 - Set hostname
 - Log everything to /var/log/aios/init.log
 - Handle signals (SIGCHLD for zombie reaping)
 - Spawn a debug shell on serial
 Build as static musl binary."
```

### Phase 4: AI Runtime
```
"Following docs/phases/04-AI-RUNTIME.md:
 Implement the aios-runtime daemon that:
 - Manages llama-server processes (start, stop, health check)
 - Exposes a gRPC inference API
 - Loads the operational model (TinyLlama) at startup
 - Auto-unloads idle tactical models after 5 minutes
 Start with the gRPC proto definition, then the Rust service."
```

### Phase 5: Agent Core (Split into 3-4 sessions)

**Session 5a: Proto + Orchestrator Skeleton**
```
"Following docs/phases/05-AGENT-CORE.md:
 1. Create all gRPC proto files (orchestrator.proto, agent.proto, common.proto)
 2. Implement the orchestrator skeleton in Rust:
    - gRPC server startup
    - Agent registration and heartbeat
    - Goal submission API
 Don't implement task planning yet, just the communication framework."
```

**Session 5b: Task Planning + Intelligence Routing**
```
"Following docs/phases/05-AGENT-CORE.md:
 Implement the task planner and intelligence routing:
 - Goal → task DAG decomposition (use local model for simple goals)
 - Intelligence level classification
 - Agent assignment based on task type
 - Result aggregation"
```

**Session 5c: Python Agent Runtime**
```
"Following docs/phases/05-AGENT-CORE.md:
 Implement the Python agent runtime:
 - BaseAgent class with gRPC client
 - SystemAgent implementation
 - All remaining agent stubs (network, security, monitor, package, storage, task, dev)
 - Agent lifecycle (register, heartbeat, execute, shutdown)"
```

**Session 5d: Integration**
```
"Wire everything together:
 - Update aios-init to start the orchestrator after runtime
 - Test: submit a goal, verify it decomposes and routes to an agent
 - Fix any integration issues"
```

### Phase 6: Tools
```
"Following docs/phases/06-TOOLS.md:
 Implement the Tool Registry service and all filesystem tools:
 - Tool registration and discovery
 - Execution pipeline (validate → perms → backup → execute → audit)
 - All 13 fs.* tools (read, write, delete, list, stat, mkdir, move, copy, chmod, chown, symlink, search, disk_usage)
 Start with the registry service, then implement tools one by one."
```

Then in a second session:
```
"Continue Phase 6: implement process.*, service.*, monitor.*, and hw.* tools.
 Also create the Python ToolClient wrapper for agents."
```

### Phases 7-12: Follow Same Pattern
Read the phase doc, break into components, implement sequentially, test after each.

---

## Key Principles for Claude Code Sessions

### 1. Always Read Before Writing
```
"Read initd/src/main.rs"         ← Do this before editing
"Read agent-core/src/main.rs"    ← Understand existing code first
```

### 2. Use Cargo Check Frequently
```
"Run cargo check --workspace"
```
Catches type errors without full compilation.

### 3. Test in QEMU Regularly
After significant changes:
```
"Build the rootfs and boot in QEMU to verify everything works"
```

### 4. Commit After Each Component
```
"Commit the goal engine implementation"
"Commit all tool implementations"
```

### 5. When Stuck, Provide Full Context
```
"I'm getting this error when trying to boot:
 [error output here]
 The relevant files are initd/src/main.rs and build/build-rootfs.sh.
 Read both files and help me debug."
```

---

## File Ownership Map

Know which files belong to which phase — helps Claude Code understand what to read:

| Component | Key Files | Phase |
|---|---|---|
| Init daemon | `initd/src/main.rs` | 3 |
| Kernel config | `kernel/configs/aios-kernel.config` | 2 |
| AI Runtime | `agent-core/src/runtime.rs` | 4 |
| Orchestrator | `agent-core/src/main.rs`, `goal_engine.rs`, `task_planner.rs` | 5 |
| Agent base | `agent-core/python/aios_agent/base.py` | 5 |
| Agent implementations | `agent-core/python/aios_agent/agents/*.py` | 5+ |
| Proto files | `agent-core/proto/*.proto` | 5 |
| Tool registry | `tools/src/main.rs`, `tools/src/registry.rs` | 6 |
| Tool implementations | `tools/src/fs/*.rs`, `tools/src/process/*.rs`, etc. | 6 |
| Memory system | `memory/src/*.rs`, `memory/python/*.py` | 7 |
| API gateway | `api-gateway/src/*.rs` | 11 |
| Build scripts | `build/*.sh` | 1, 12 |
| System config | `etc/aios/config.toml` (in rootfs) | 3 |
| Security | `security/src/*.rs` | 9 |

---

## Debugging Tips for Claude Code

### Kernel Won't Boot
```
"The kernel panics on boot. Read the QEMU output I'm about to paste,
 and also read kernel/configs/aios-kernel.config. Tell me what's missing."
```

### gRPC Services Won't Connect
```
"The orchestrator can't connect to the tool registry.
 Read agent-core/src/main.rs and tools/src/main.rs.
 Check the socket paths and gRPC setup."
```

### Agent Can't Call Tools
```
"The System Agent gets a permission denied error when calling fs.read.
 Read security/src/capabilities.rs and the agent's config in etc/aios/agents/system.toml.
 Check the capability grants."
```

### Model Won't Load
```
"The AI runtime fails to load TinyLlama on boot.
 Read the runtime logs and agent-core/src/runtime.rs.
 Check the model path and llama-server arguments."
```

---

## Cost Optimization

Claude Code sessions cost API tokens. Optimize by:

1. **Read docs before coding** — reduces back-and-forth
2. **Be specific in prompts** — "implement fs.write in tools/src/fs/write.rs" > "write some file tools"
3. **Batch related changes** — do all fs.* tools in one session
4. **Use cargo check, not cargo build** — faster feedback
5. **Keep context focused** — don't load files you don't need

---

## Multi-Session Continuity

At the end of each session, commit your changes. At the start of the next session:

```
"We're continuing the aiOS build. I just finished Phase 5 (agent core).
 Read CLAUDE.md for project context, then read docs/phases/06-TOOLS.md
 for what we're building next. Also check what's currently in tools/src/
 to see what already exists."
```

This gives Claude Code full context to continue where you left off.
