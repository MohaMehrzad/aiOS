# Tool Registry Architecture

## Overview

The Tool Registry is the interface between AI agents and the operating system. Every system operation — reading a file, starting a process, opening a port — is a **tool** with a typed schema, permission model, audit trail, and rollback capability.

Agents NEVER run raw shell commands. They ONLY call tools.

---

## Tool Anatomy

Every tool has this structure:

```python
@dataclass
class ToolDefinition:
    # Identity
    name: str                    # e.g., "fs.read"
    namespace: str               # e.g., "fs"
    version: str                 # e.g., "1.0.0"
    description: str             # Human/AI readable description

    # Schema
    input_schema: JSONSchema     # Typed input parameters
    output_schema: JSONSchema    # Typed output

    # Security
    required_capabilities: list[str]  # e.g., ["fs:read"]
    risk_level: str              # "low", "medium", "high", "critical"
    requires_confirmation: bool  # Orchestrator must approve before execution

    # Behavior
    idempotent: bool             # Safe to retry
    reversible: bool             # Can be rolled back
    timeout_ms: int              # Max execution time
    rollback_tool: str | None    # Tool to call for rollback (e.g., "fs.delete" for "fs.write")
```

---

## Tool Namespaces

### `fs.*` — Filesystem Operations

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `fs.read` | Read file contents | low | n/a |
| `fs.write` | Write/create file | medium | yes (backup original) |
| `fs.delete` | Delete file | high | yes (soft delete first) |
| `fs.list` | List directory contents | low | n/a |
| `fs.stat` | Get file metadata | low | n/a |
| `fs.mkdir` | Create directory | low | yes |
| `fs.move` | Move/rename file | medium | yes |
| `fs.copy` | Copy file | low | yes |
| `fs.chmod` | Change permissions | medium | yes |
| `fs.chown` | Change ownership | medium | yes |
| `fs.symlink` | Create symbolic link | low | yes |
| `fs.search` | Search for files by pattern | low | n/a |
| `fs.disk_usage` | Get disk usage stats | low | n/a |

**Example: `fs.write`**
```json
{
  "name": "fs.write",
  "input": {
    "path": "/etc/nginx/nginx.conf",
    "content": "server { listen 80; ... }",
    "mode": "0644",
    "backup": true
  },
  "output": {
    "success": true,
    "bytes_written": 1234,
    "backup_path": "/var/lib/aios/backups/etc_nginx_nginx.conf.20240101T120000"
  }
}
```

### `process.*` — Process Management

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `process.spawn` | Start a new process | medium | yes (kill it) |
| `process.kill` | Kill a process | high | no |
| `process.list` | List running processes | low | n/a |
| `process.info` | Get process details | low | n/a |
| `process.signal` | Send signal to process | medium | no |
| `process.wait` | Wait for process to exit | low | n/a |

### `service.*` — Service Lifecycle

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `service.start` | Start a service | medium | yes (stop it) |
| `service.stop` | Stop a service | medium | yes (start it) |
| `service.restart` | Restart a service | medium | yes |
| `service.status` | Get service status | low | n/a |
| `service.enable` | Enable service auto-start | low | yes |
| `service.disable` | Disable service auto-start | low | yes |
| `service.logs` | Get service logs | low | n/a |

### `net.*` — Networking

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `net.interfaces` | List network interfaces | low | n/a |
| `net.configure` | Configure interface (IP, DNS) | high | yes (save prev config) |
| `net.check_port` | Check if port is open/used | low | n/a |
| `net.http_request` | Make HTTP request | low | n/a |
| `net.dns_lookup` | DNS lookup | low | n/a |
| `net.ping` | Ping a host | low | n/a |
| `net.listen` | Start listening on port | medium | yes (stop listening) |
| `net.connections` | List active connections | low | n/a |

### `firewall.*` — Firewall Management

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `firewall.rules` | List firewall rules | low | n/a |
| `firewall.allow` | Allow traffic (add rule) | medium | yes (remove rule) |
| `firewall.deny` | Deny traffic (add rule) | high | yes (remove rule) |
| `firewall.remove` | Remove firewall rule | high | yes (re-add rule) |

### `pkg.*` — Package Management

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `pkg.search` | Search for packages | low | n/a |
| `pkg.install` | Install package | medium | yes (uninstall) |
| `pkg.remove` | Remove package | high | partial (reinstall) |
| `pkg.update` | Update package | medium | partial (downgrade) |
| `pkg.list` | List installed packages | low | n/a |
| `pkg.info` | Get package details | low | n/a |

### `sec.*` — Security

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `sec.grant` | Grant capability to agent | critical | yes (revoke) |
| `sec.revoke` | Revoke capability | medium | yes (re-grant) |
| `sec.audit` | Run security audit | low | n/a |
| `sec.scan` | Vulnerability scan | low | n/a |
| `sec.cert_generate` | Generate TLS certificate | low | yes |
| `sec.cert_rotate` | Rotate certificate | medium | yes (revert) |

### `monitor.*` — Monitoring

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `monitor.cpu` | Get CPU usage | low | n/a |
| `monitor.memory` | Get memory usage | low | n/a |
| `monitor.disk` | Get disk usage | low | n/a |
| `monitor.network` | Get network stats | low | n/a |
| `monitor.gpu` | Get GPU stats | low | n/a |
| `monitor.health` | System health check | low | n/a |
| `monitor.watch` | Set up metric watcher | low | yes (unwatch) |

### `hw.*` — Hardware

| Tool | Description | Risk | Reversible |
|---|---|---|---|
| `hw.info` | Get hardware information | low | n/a |
| `hw.cpu` | CPU details | low | n/a |
| `hw.memory` | Memory details | low | n/a |
| `hw.gpu` | GPU details | low | n/a |
| `hw.storage` | Storage device details | low | n/a |
| `hw.network` | Network device details | low | n/a |

---

## Tool Execution Pipeline

```
Agent calls tool
       │
       ▼
┌──────────────┐
│ 1. VALIDATE  │  Check input against JSON schema
│    INPUT     │  Reject malformed requests immediately
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ 2. CHECK     │  Does the calling agent have required capabilities?
│    PERMS     │  Reject if insufficient permissions
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ 3. CONFIRM   │  If risk_level >= high, ask orchestrator for approval
│    (if needed)│  Orchestrator may ask human via management console
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ 4. PRE-EXEC  │  Create backup/snapshot if tool is reversible
│    BACKUP    │  Record pre-state for rollback
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ 5. EXECUTE   │  Run the actual operation
│              │  Apply timeout
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ 6. VALIDATE  │  Check output against schema
│    OUTPUT    │  Verify operation succeeded
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ 7. AUDIT     │  Log: who, what, when, why, result
│    LOG       │  Store in memory system
└──────┬───────┘
       │
       ▼
Return result to agent
```

---

## Tool Registry Service

The Tool Registry runs as a gRPC service (`aios-tools`).

### gRPC API

```protobuf
service ToolRegistry {
    // List all available tools
    rpc ListTools(ListToolsRequest) returns (ListToolsResponse);

    // Get tool definition
    rpc GetTool(GetToolRequest) returns (ToolDefinition);

    // Execute a tool
    rpc Execute(ExecuteRequest) returns (ExecuteResponse);

    // Rollback a previous execution
    rpc Rollback(RollbackRequest) returns (RollbackResponse);

    // Register a new tool (for plugins)
    rpc Register(RegisterRequest) returns (RegisterResponse);
}

message ExecuteRequest {
    string tool_name = 1;
    string agent_id = 2;
    string task_id = 3;
    bytes input_json = 4;        // JSON input matching tool's input_schema
    string reason = 5;           // Why the agent is calling this tool
}

message ExecuteResponse {
    bool success = 1;
    bytes output_json = 2;       // JSON output matching tool's output_schema
    string error = 3;
    string execution_id = 4;     // For rollback reference
    int64 duration_ms = 5;
}
```

---

## Tool Implementation Pattern

Each tool is implemented as a Rust function with a Python wrapper:

```rust
// Rust: tools/src/fs/read.rs
pub async fn fs_read(input: FsReadInput) -> Result<FsReadOutput> {
    // Validate path is not in restricted list
    security::check_path(&input.path)?;

    // Read file
    let content = tokio::fs::read_to_string(&input.path).await?;

    Ok(FsReadOutput {
        content,
        size: content.len(),
        modified: fs::metadata(&input.path).await?.modified()?,
    })
}
```

```python
# Python: tools/python/fs.py (for agent-side convenience)
async def fs_read(path: str) -> FsReadResult:
    """Read a file's contents."""
    response = await tool_client.execute(
        tool_name="fs.read",
        input={"path": path}
    )
    return FsReadResult(**response.output)
```

---

## Custom Tool Registration

Agents or plugins can register new tools at runtime:

```python
await tool_registry.register(
    name="custom.my_tool",
    description="Does something custom",
    input_schema={...},
    output_schema={...},
    handler=my_handler_function,
    required_capabilities=["custom:my_tool"],
    risk_level="medium"
)
```

This allows aiOS to be extended without modifying core code.

---

## Audit Log Format

Every tool execution produces an audit entry:

```json
{
  "execution_id": "exec-20240101-123456-abcdef",
  "timestamp": "2024-01-01T12:34:56.789Z",
  "tool": "fs.write",
  "agent": "aios-agent-system",
  "task_id": "task-789",
  "goal_id": "goal-456",
  "input": {"path": "/etc/nginx/nginx.conf", "content": "..."},
  "output": {"success": true, "bytes_written": 1234},
  "reason": "Configuring nginx as reverse proxy for goal-456",
  "duration_ms": 45,
  "risk_level": "medium",
  "backup_path": "/var/lib/aios/backups/...",
  "capabilities_used": ["fs:write"]
}
```
