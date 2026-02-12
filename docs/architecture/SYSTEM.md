# System Architecture

## Layer Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                    MANAGEMENT CONSOLE                         │
│              (Human oversight — web UI / SSH)                 │
└──────────────────────┬───────────────────────────────────────┘
                       │ Management API (HTTPS/gRPC)
┌──────────────────────▼───────────────────────────────────────┐
│                   EXTERNAL API GATEWAY                        │
│         Claude API │ OpenAI API │ Custom Endpoints            │
│         Rate limiting │ Fallback │ Cost tracking              │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                     AI ORCHESTRATOR                           │
│  ┌─────────┐ ┌──────────┐ ┌──────────┐ ┌────────────────┐   │
│  │  Goal    │ │  Task    │ │  Agent   │ │  Decision      │   │
│  │  Engine  │ │  Planner │ │  Router  │ │  Logger        │   │
│  └─────────┘ └──────────┘ └──────────┘ └────────────────┘   │
└──────────────────────┬───────────────────────────────────────┘
                       │ gRPC / Unix Domain Sockets
┌──────────────────────▼───────────────────────────────────────┐
│                     AGENT MESH                                │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │ System   │ │ Network  │ │ Security │ │ Task     │        │
│  │ Agent    │ │ Agent    │ │ Agent    │ │ Agent    │        │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │ Package  │ │ Storage  │ │ Monitor  │ │ Dev      │        │
│  │ Agent    │ │ Agent    │ │ Agent    │ │ Agent    │        │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
└──────────────────────┬───────────────────────────────────────┘
                       │ Tool Calls (typed, audited)
┌──────────────────────▼───────────────────────────────────────┐
│                    TOOL REGISTRY                              │
│   fs.* │ process.* │ net.* │ pkg.* │ sec.* │ hw.*           │
│   Each tool: typed schema + permissions + audit + rollback    │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                    MEMORY SYSTEM                              │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐     │
│  │ Operational   │ │ Working      │ │ Long-term         │     │
│  │ (in-memory)   │ │ (SQLite)     │ │ (ChromaDB+SQLite) │     │
│  └──────────────┘ └──────────────┘ └──────────────────┘     │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                  LOCAL AI RUNTIME                             │
│   llama.cpp server │ Model Manager │ Inference Queue          │
│   Models: TinyLlama 1.1B, Phi-3 3.8B, Mistral 7B, Llama 13B │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                   AI INIT DAEMON (PID 1)                     │
│        Boot → Load models → Start agents → Autonomy loop     │
└──────────────────────┬───────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                   LINUX KERNEL 6.x                            │
│     Minimal config │ GPU drivers │ Network │ Filesystem       │
└──────────────────────────────────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────────┐
│                      HARDWARE                                 │
│            CPU │ RAM │ GPU │ NVMe │ NIC                       │
└──────────────────────────────────────────────────────────────┘
```

---

## Data Flow: A Request Lifecycle

Example: "Install nginx and configure it as a reverse proxy"

```
1. MANAGEMENT CONSOLE
   └─> Human sends goal via API

2. AI ORCHESTRATOR — Goal Engine
   └─> Receives goal: "Install nginx and configure it as a reverse proxy"
   └─> Sends to Task Planner

3. AI ORCHESTRATOR — Task Planner
   └─> Calls Claude API (strategic layer) for plan decomposition
   └─> Claude returns plan:
       Task 1: Check if nginx is available in package repos
       Task 2: Install nginx package
       Task 3: Generate reverse proxy config based on running services
       Task 4: Write config file
       Task 5: Start nginx service
       Task 6: Verify nginx is running and proxying correctly
       Task 7: Add nginx to monitored services

4. AI ORCHESTRATOR — Agent Router
   └─> Task 1 → Package Agent (local 7B model can handle)
   └─> Task 2 → Package Agent (tool call)
   └─> Task 3 → Claude API (needs reasoning about config)
   └─> Task 4 → System Agent (tool call)
   └─> Task 5 → System Agent (tool call)
   └─> Task 6 → Monitor Agent (tool call + local model analysis)
   └─> Task 7 → Monitor Agent (tool call)

5. AGENT MESH — Package Agent
   └─> Calls tool: pkg.search("nginx")
   └─> TOOL REGISTRY executes, returns result
   └─> Agent confirms: nginx available, version 1.24.0
   └─> Calls tool: pkg.install("nginx")
   └─> TOOL REGISTRY executes installation
   └─> All actions logged to MEMORY SYSTEM

6. AGENT MESH — System Agent
   └─> Receives config from Claude API response
   └─> Calls tool: fs.write("/etc/nginx/sites-available/proxy.conf", config)
   └─> Calls tool: fs.symlink("/etc/nginx/sites-enabled/proxy.conf", ...)
   └─> Calls tool: process.start("nginx")

7. AGENT MESH — Monitor Agent
   └─> Calls tool: net.check_port(80)
   └─> Calls tool: net.http_request("http://localhost")
   └─> Uses local model to analyze response: "Is this working correctly?"
   └─> Confirms success, adds to monitoring watchlist

8. AI ORCHESTRATOR — Decision Logger
   └─> Logs complete execution trace
   └─> Records: goal → plan → tasks → results → outcome
   └─> Updates long-term memory with pattern: "nginx reverse proxy setup"

9. MANAGEMENT CONSOLE
   └─> Human sees: "Goal completed. nginx installed and configured as reverse proxy."
   └─> Full execution trace available for review
```

---

## Component Interaction Map

```
                    ┌─────────────┐
                    │  Claude API │
                    │  OpenAI API │
                    └──────┬──────┘
                           │
┌──────────┐       ┌──────▼──────┐       ┌──────────┐
│ Management├──────►│ Orchestrator├──────►│  Memory  │
│ Console   │       └──────┬──────┘       │  System  │
└──────────┘              │               └────┬─────┘
                    ┌─────▼──────┐              │
                    │ Agent Mesh │◄─────────────┘
                    └─────┬──────┘
                          │
                    ┌─────▼──────┐
                    │   Tool     │
                    │  Registry  │
                    └─────┬──────┘
                          │
              ┌───────────┼───────────┐
              ▼           ▼           ▼
         ┌────────┐ ┌────────┐ ┌────────┐
         │Filesys │ │Process │ │Network │
         │  Ops   │ │  Ops   │ │  Ops   │
         └────────┘ └────────┘ └────────┘
```

---

## IPC Architecture

All inter-component communication uses **gRPC** with Protocol Buffers for structured, typed messaging.

### Why gRPC
- Typed contracts (no string parsing)
- Bidirectional streaming (agents can push updates)
- Language agnostic (Rust services talk to Python services)
- Built-in deadline/timeout support
- Code generation for both Rust (tonic) and Python (grpcio)

### Communication Patterns

| Pattern | Used For | Example |
|---|---|---|
| Request/Response | Tool calls, queries | Agent calls `fs.read("/etc/hostname")` |
| Server Streaming | Log watching, monitoring | Monitor agent streams system metrics |
| Bidirectional Streaming | Agent-to-agent collaboration | System and network agent coordinate service setup |
| Fire-and-forget | Audit logging, metrics | Log every tool call to the ledger |

### Transport
- **Local**: Unix domain sockets (`/run/aios/orchestrator.sock`)
- **Remote management**: mTLS gRPC on port 9090
- **Internal HTTP**: localhost only, no external exposure

---

## Process Hierarchy

```
PID 1: aios-init (AI Init Daemon)
├── PID 2: aios-runtime (Local AI Model Server)
│   ├── Model: tinyllama-1.1b (always loaded, operational layer)
│   ├── Model: phi-3-3.8b (loaded on demand, tactical layer)
│   └── Model: mistral-7b (loaded on demand, tactical layer)
├── PID 3: aios-orchestrator (AI Orchestrator)
├── PID 4: aios-memory (Memory System Daemon)
├── PID 5: aios-tools (Tool Registry Service)
├── Agents (spawned by orchestrator):
│   ├── aios-agent-system
│   ├── aios-agent-network
│   ├── aios-agent-security
│   ├── aios-agent-monitor
│   ├── aios-agent-package
│   ├── aios-agent-storage
│   ├── aios-agent-task
│   └── aios-agent-dev
└── Task processes (spawned by agents):
    ├── nginx, postgres, etc. (managed services)
    └── sandboxed workloads (in Podman containers)
```

---

## Filesystem Layout

```
/
├── bin/                    Symlink to /usr/bin
├── sbin/                   Symlink to /usr/sbin
├── usr/
│   ├── bin/                System binaries (BusyBox, coreutils)
│   ├── sbin/               System admin binaries
│   ├── lib/                Shared libraries
│   └── share/              Shared data files
├── etc/
│   ├── aios/               aiOS configuration
│   │   ├── config.toml     Main system configuration
│   │   ├── agents/         Per-agent configuration
│   │   ├── tools/          Tool definitions and permissions
│   │   └── models/         Model configurations
│   ├── network/            Network configuration
│   └── security/           Security policies
├── var/
│   ├── lib/
│   │   └── aios/
│   │       ├── models/     AI model files (GGUF)
│   │       ├── memory/     SQLite databases
│   │       ├── vectors/    ChromaDB vector store
│   │       ├── cache/      Inference and tool caches
│   │       └── ledger/     Audit ledger
│   ├── log/
│   │   └── aios/           System and agent logs
│   └── run/
│       └── aios/           Runtime sockets and PID files
├── run/                    tmpfs, runtime data
│   └── aios/
│       ├── orchestrator.sock
│       ├── tools.sock
│       ├── memory.sock
│       └── agents/         Per-agent sockets
├── tmp/                    tmpfs, temporary files
└── home/
    └── workspaces/         Task workspaces (created/destroyed per task)
```

---

## Configuration System

All configuration lives in `/etc/aios/config.toml`:

```toml
[system]
hostname = "aios-node-01"
log_level = "info"
autonomy_level = "full"          # full | supervised | manual

[models]
runtime = "llama-cpp"
model_dir = "/var/lib/aios/models"

[models.operational]
name = "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
always_loaded = true
max_tokens = 512
temperature = 0.1

[models.tactical]
name = "mistral-7b-instruct-v0.3.Q4_K_M.gguf"
load_on_demand = true
max_tokens = 2048
temperature = 0.3
unload_after_idle = "5m"

[api.claude]
model = "claude-sonnet-4-5-20250929"
max_tokens = 4096
monthly_budget_usd = 100.0
rate_limit_rpm = 50

[api.openai]
model = "gpt-4o"
max_tokens = 4096
monthly_budget_usd = 50.0
fallback_only = true             # Only use when Claude is unavailable

[memory]
operational_max_entries = 10000
working_db = "/var/lib/aios/memory/working.db"
longterm_db = "/var/lib/aios/memory/longterm.db"
vector_db = "/var/lib/aios/vectors"

[security]
capability_mode = "strict"
audit_all_tool_calls = true
sandbox_untrusted = true
auto_patch = true

[network]
management_port = 9090
management_tls = true
allow_outbound = true
dns_servers = ["1.1.1.1", "8.8.8.8"]
```

---

## Boot Sequence

```
1. BIOS/UEFI → GRUB → Linux Kernel
2. Kernel initializes hardware, mounts root filesystem
3. Kernel starts PID 1: /usr/sbin/aios-init

4. aios-init Phase 1: HARDWARE CHECK
   - Enumerate CPU, RAM, GPU, storage, network
   - Verify minimum requirements met
   - Mount filesystems (/var, /tmp, /run)
   - Set up tmpfs for /run/aios

5. aios-init Phase 2: AI RUNTIME
   - Start aios-runtime (llama.cpp server)
   - Load operational model (TinyLlama 1.1B) — MUST succeed
   - Health check: send test prompt, verify response
   - If GPU available: initialize CUDA/ROCm runtime

6. aios-init Phase 3: CORE SERVICES
   - Start aios-memory (memory system daemon)
   - Start aios-tools (tool registry)
   - Start aios-orchestrator (AI orchestrator)
   - Each service registers with orchestrator

7. aios-init Phase 4: AGENT SPAWN
   - Orchestrator reads agent configs from /etc/aios/agents/
   - Spawns all configured agents
   - Each agent registers its capabilities with orchestrator
   - System agent performs initial system health check

8. aios-init Phase 5: AUTONOMY
   - All systems online — orchestrator enters autonomy loop
   - Loads pending goals from memory
   - Begins continuous monitoring cycle
   - Management console becomes available on port 9090
   - System is READY

Total boot time target: <30 seconds to autonomy loop
```
