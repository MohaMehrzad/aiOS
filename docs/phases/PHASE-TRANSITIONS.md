# Phase Transition Guide

## Overview

Each phase modifies `aios-init` to start additional services. This document shows the exact changes needed to `initd/src/main.rs` at each phase boundary.

---

## aios-init Evolution Across Phases

### Phase 3: Stub Init
```
PID 1: aios-init
  └── Phase 1: Mount filesystems
  └── Phase 2: Detect hardware
  └── [STOP] Spawn debug shell
```

### Phase 4: Add AI Runtime
```
PID 1: aios-init
  └── Phase 1: Mount filesystems
  └── Phase 2: Detect hardware
  └── Phase 3: Start aios-runtime          ← NEW
  │     └── Load operational model
  │     └── Health check: send test prompt
  └── [STOP] Spawn debug shell
```

**Code change in initd/src/main.rs:**
```rust
// After hardware detection, add:
info!("Phase 2: Starting AI Runtime...");
let runtime = ServiceSupervisor::start("aios-runtime", "/usr/sbin/aios-runtime", &config)?;
runtime.wait_for_health(Duration::from_secs(30))?;
info!("AI Runtime online, operational model loaded");
```

### Phase 5: Add Orchestrator
```
PID 1: aios-init
  └── Phase 1: Mount filesystems
  └── Phase 2: Detect hardware
  └── Phase 3: Start aios-runtime
  └── Phase 4: Start aios-orchestrator     ← NEW
  │     └── Load agent configs
  │     └── Spawn agents
  │     └── Enter autonomy loop
  └── [STOP] Spawn debug shell (if debug_shell=true)
```

**Code change:**
```rust
info!("Phase 3: Starting Orchestrator...");
let orchestrator = ServiceSupervisor::start(
    "aios-orchestrator", "/usr/sbin/aios-orchestrator", &config
)?;
orchestrator.wait_for_health(Duration::from_secs(10))?;
info!("Orchestrator online");
```

### Phase 6: Add Tool Registry
```
PID 1: aios-init
  └── Phase 1: Mount filesystems
  └── Phase 2: Detect hardware
  └── Phase 3: Start aios-runtime
  └── Phase 3b: Start aios-tools           ← NEW (before orchestrator)
  └── Phase 4: Start aios-orchestrator
  └── ...
```

**Key insight**: Tool registry MUST start before orchestrator, because agents need it to function.

### Phase 7: Add Memory Service
```
PID 1: aios-init
  └── Phase 1: Mount filesystems
  └── Phase 2: Detect hardware + decrypt secrets
  └── Phase 3: Start aios-runtime
  └── Phase 3b: Start aios-memory          ← NEW (before tools and orchestrator)
  └── Phase 3c: Start aios-tools
  └── Phase 4: Start aios-orchestrator
  └── ...
```

**Key insight**: Memory service starts before tools (tools log to memory) and orchestrator (orchestrator reads memory for pending goals).

### Final Boot Order (Phase 12)

```
PID 1: aios-init
  ├── Phase 1: HARDWARE
  │   ├── Mount /proc, /sys, /dev, /tmp, /run
  │   ├── Detect CPU, RAM, GPU, storage, network
  │   └── Read /etc/aios/config.toml
  │
  ├── Phase 2: SECRETS
  │   ├── Decrypt secrets.enc → kernel keyring
  │   └── Delete secrets.enc from disk
  │
  ├── Phase 3: AI RUNTIME
  │   ├── Start aios-runtime daemon
  │   ├── Load operational model (TinyLlama)
  │   └── Health check: test prompt → response
  │
  ├── Phase 4: CORE SERVICES
  │   ├── Start aios-memory (memory system)
  │   ├── Start aios-tools (tool registry)
  │   └── Wait for both to report healthy
  │
  ├── Phase 5: ORCHESTRATOR
  │   ├── Start aios-orchestrator
  │   ├── Orchestrator loads agent configs from /etc/aios/agents/
  │   ├── Orchestrator spawns all enabled agents
  │   └── Each agent registers with orchestrator
  │
  ├── Phase 6: RECOVERY CHECK
  │   ├── Check for clean_shutdown flag
  │   ├── If unclean: run recovery procedure
  │   └── Resume interrupted goals
  │
  └── Phase 7: AUTONOMY
      ├── All systems online
      ├── Management console available on port 9090
      ├── Create clean_shutdown flag
      └── Enter supervisor loop (monitor all services)
```

---

## Service Startup Dependencies

```
aios-runtime     → depends on: nothing (first service)
aios-memory      → depends on: nothing (standalone, but benefits from runtime for embeddings)
aios-tools       → depends on: aios-memory (for audit logging)
aios-orchestrator → depends on: aios-runtime, aios-memory, aios-tools
agents           → depends on: aios-orchestrator, aios-tools, aios-runtime
management       → depends on: aios-orchestrator (embedded in it)
```

---

## Verification at Each Transition

### Phase 3 → 4: "Can I get a response from the local model?"
```bash
# Inside QEMU, after boot:
# The runtime should be listening on port 8080 (llama-server default)
curl http://localhost:8080/health
# Expected: {"status":"ok"}

curl -X POST http://localhost:8080/completion \
    -d '{"prompt":"Say OK","n_predict":5}'
# Expected: response containing "OK"
```

### Phase 4 → 5: "Can an agent receive and execute a task?"
```bash
# Use grpcurl to test orchestrator
grpcurl -plaintext unix:///run/aios/orchestrator.sock \
    aios.orchestrator.Orchestrator/SubmitGoal \
    -d '{"description":"Echo test: confirm system is working"}'
# Expected: goal_id returned
```

### Phase 5 → 6: "Can an agent call a tool?"
```bash
# The system agent should be able to call fs.read
# Submit a goal that requires a tool call
grpcurl -plaintext unix:///run/aios/orchestrator.sock \
    aios.orchestrator.Orchestrator/SubmitGoal \
    -d '{"description":"Read the contents of /etc/hostname and report"}'
# Expected: goal completes with hostname content
```

### Phase 6 → 7: "Does memory persist across operations?"
```bash
# Submit two goals and verify the second remembers the first
# Goal 1: "Remember that the password policy requires 12 characters"
# Goal 2: "What is the password policy?"
# Expected: Goal 2 references the memory from Goal 1
```
