# Phase 6: Tool Registry & System Tools

## Goal
Build the Tool Registry service and implement all core system tools (30+). After this phase, agents can perform real system operations through structured, audited, reversible tool calls.

## Prerequisites
- Phase 5 complete (orchestrator + agents communicating)
- Read [architecture/TOOL-REGISTRY.md](../architecture/TOOL-REGISTRY.md)

---

## Step-by-Step

### Step 6.1: Implement Tool Registry Service (Rust)

**Claude Code prompt**: "Implement the aios-tools gRPC service in Rust — tool registration, discovery, execution pipeline (validate → check perms → backup → execute → audit)"

```
File: tools/src/main.rs + submodules

tools/src/
├── main.rs           — gRPC server startup
├── registry.rs       — Tool registration and discovery
├── executor.rs       — Execution pipeline (validate, perms, execute, audit)
├── audit.rs          — Audit logging
├── backup.rs         — Pre-execution backups for reversible tools
├── schema.rs         — JSON schema validation
├── fs/               — Filesystem tools
│   ├── mod.rs
│   ├── read.rs
│   ├── write.rs
│   ├── delete.rs
│   ├── list.rs
│   ├── stat.rs
│   ├── mkdir.rs
│   ├── move_file.rs
│   ├── copy.rs
│   ├── chmod.rs
│   ├── chown.rs
│   ├── symlink.rs
│   ├── search.rs
│   └── disk_usage.rs
├── process/          — Process tools
│   ├── mod.rs
│   ├── spawn.rs
│   ├── kill.rs
│   ├── list.rs
│   ├── info.rs
│   └── signal.rs
├── service/          — Service tools
│   ├── mod.rs
│   ├── start.rs
│   ├── stop.rs
│   ├── restart.rs
│   ├── status.rs
│   └── logs.rs
├── net/              — Network tools (basics, full network in Phase 8)
│   ├── mod.rs
│   ├── interfaces.rs
│   ├── check_port.rs
│   ├── http_request.rs
│   ├── dns_lookup.rs
│   └── ping.rs
├── monitor/          — Monitoring tools
│   ├── mod.rs
│   ├── cpu.rs
│   ├── memory.rs
│   ├── disk.rs
│   ├── gpu.rs
│   └── health.rs
└── hw/               — Hardware info tools
    ├── mod.rs
    └── info.rs
```

### Step 6.2: Implement Execution Pipeline

**Claude Code prompt**: "Implement the tool execution pipeline: input validation, permission checking, pre-execution backup, execution, output validation, and audit logging"

```rust
// tools/src/executor.rs

pub async fn execute_tool(request: ExecuteRequest) -> Result<ExecuteResponse> {
    let tool = registry.get(&request.tool_name)?;
    let start = Instant::now();

    // 1. Validate input
    schema::validate(&tool.input_schema, &request.input_json)?;

    // 2. Check permissions
    let agent_caps = security::get_agent_capabilities(&request.agent_id).await?;
    security::check_capabilities(&tool.required_capabilities, &agent_caps)?;

    // 3. Confirmation check (high-risk tools)
    if tool.risk_level >= RiskLevel::High {
        orchestrator::request_confirmation(&request).await?;
    }

    // 4. Pre-execution backup (reversible tools)
    let backup_id = if tool.reversible {
        Some(backup::create_backup(&request).await?)
    } else {
        None
    };

    // 5. Execute
    let result = tool.handler.execute(&request.input_json).await;

    // 6. Validate output
    if let Ok(ref output) = result {
        schema::validate(&tool.output_schema, output)?;
    }

    // 7. Audit log
    audit::log(AuditEntry {
        execution_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        tool: request.tool_name.clone(),
        agent: request.agent_id.clone(),
        task_id: request.task_id.clone(),
        input: request.input_json.clone(),
        output: result.as_ref().ok().cloned(),
        success: result.is_ok(),
        duration_ms: start.elapsed().as_millis() as u64,
        reason: request.reason.clone(),
        risk_level: tool.risk_level.to_string(),
        backup_id,
    }).await?;

    // 8. Return result
    match result {
        Ok(output) => Ok(ExecuteResponse {
            success: true,
            output_json: output,
            execution_id: audit_entry.execution_id,
            duration_ms: start.elapsed().as_millis() as i64,
            ..Default::default()
        }),
        Err(e) => Ok(ExecuteResponse {
            success: false,
            error: e.to_string(),
            ..Default::default()
        }),
    }
}
```

### Step 6.3: Implement Filesystem Tools

**Claude Code prompt**: "Implement all fs.* tools — read, write, delete, list, stat, mkdir, move, copy, chmod, chown, symlink, search, disk_usage. Each tool must handle errors gracefully and log operations."

Example implementation:
```rust
// tools/src/fs/write.rs
pub async fn fs_write(input: Value) -> Result<Value> {
    let path: &str = input["path"].as_str().ok_or(anyhow!("path required"))?;
    let content: &str = input["content"].as_str().ok_or(anyhow!("content required"))?;
    let mode: u32 = input["mode"].as_str()
        .map(|m| u32::from_str_radix(m, 8).unwrap_or(0o644))
        .unwrap_or(0o644);
    let backup: bool = input["backup"].as_bool().unwrap_or(true);

    // Create backup if file exists and backup requested
    let backup_path = if backup && Path::new(path).exists() {
        let bp = format!("/var/lib/aios/backups/{}.{}",
            path.replace('/', "_"),
            Utc::now().format("%Y%m%dT%H%M%S"));
        tokio::fs::copy(path, &bp).await?;
        Some(bp)
    } else {
        None
    };

    // Ensure parent directory exists
    if let Some(parent) = Path::new(path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Write file
    tokio::fs::write(path, content).await?;

    // Set permissions
    tokio::fs::set_permissions(path, Permissions::from_mode(mode)).await?;

    Ok(json!({
        "success": true,
        "bytes_written": content.len(),
        "backup_path": backup_path,
    }))
}
```

### Step 6.4: Implement Process Tools

**Claude Code prompt**: "Implement all process.* tools — spawn, kill, list, info, signal"

### Step 6.5: Implement Service Tools

**Claude Code prompt**: "Implement all service.* tools — start, stop, restart, status, logs. Services are tracked by the orchestrator; these tools manage the actual processes."

### Step 6.6: Implement Monitoring Tools

**Claude Code prompt**: "Implement all monitor.* tools — cpu, memory, disk, gpu, health. Read from /proc and /sys to gather real system metrics."

### Step 6.7: Implement Hardware Info Tools

**Claude Code prompt**: "Implement hw.info tool that returns complete hardware inventory by reading /proc/cpuinfo, /proc/meminfo, /sys/class/*, and lspci output"

### Step 6.8: Create Python Tool Client

**Claude Code prompt**: "Create a Python client library that agents use to call tools, with async support and typed wrappers for common operations"

```python
# tools/python/tool_client.py

class ToolClient:
    """Async gRPC client for calling tools from Python agents."""

    async def execute(self, tool_name: str, agent_id: str,
                      task_id: str, input: dict, reason: str) -> ToolResult:
        """Execute a tool and return the result."""
        response = await self._stub.Execute(ExecuteRequest(
            tool_name=tool_name,
            agent_id=agent_id,
            task_id=task_id,
            input_json=json.dumps(input).encode(),
            reason=reason,
        ))
        return ToolResult(
            success=response.success,
            data=json.loads(response.output_json) if response.output_json else None,
            error=response.error or None,
            execution_id=response.execution_id,
        )

    # Convenience methods
    async def read_file(self, path: str) -> str:
        result = await self.execute("fs.read", input={"path": path})
        return result.data["content"]

    async def write_file(self, path: str, content: str) -> bool:
        result = await self.execute("fs.write", input={"path": path, "content": content})
        return result.success

    async def list_processes(self) -> list[dict]:
        result = await self.execute("process.list", input={})
        return result.data["processes"]
```

### Step 6.9: Integration Test

**Claude Code prompt**: "Test all tools end-to-end: agent submits tool calls, registry validates, executes, audits, and returns results. Test backup/rollback for fs.write."

---

## Deliverables Checklist

- [ ] Tool Registry gRPC service starts and accepts connections
- [ ] Execution pipeline (validate → perms → execute → audit) works
- [ ] All 13 `fs.*` tools implemented and tested
- [ ] All 5 `process.*` tools implemented and tested
- [ ] All 5 `service.*` tools implemented and tested
- [ ] All 5 `monitor.*` tools implemented and tested
- [ ] `hw.info` tool implemented and tested
- [ ] Audit log records every tool call
- [ ] Backup/rollback works for reversible tools
- [ ] Python ToolClient works from agent code
- [ ] 30+ tools total operational

---

## Next Phase
→ [Phase 7: Memory System](./07-MEMORY.md) (can be parallel with Phase 6)
