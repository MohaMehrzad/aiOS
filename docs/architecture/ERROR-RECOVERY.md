# Error Recovery & Watchdog Architecture

## Overview

aiOS must never stop. When things go wrong — and they will — the system must detect, recover, and learn. This document defines the error recovery strategy at every level.

---

## Recovery Hierarchy

```
Level 0: Hardware watchdog timer (kernel resets machine if PID 1 dies)
Level 1: aios-init monitors all core services
Level 2: Orchestrator monitors all agents
Level 3: Agents handle their own task-level errors
Level 4: Tool-level retry and rollback
```

---

## Level 0: Hardware / Kernel Watchdog

If `aios-init` (PID 1) crashes, the kernel panics. For bare-metal deployments:

```
# In kernel config
CONFIG_WATCHDOG=y
CONFIG_SOFT_WATCHDOG=y

# aios-init pings the watchdog every 30 seconds
# If it stops pinging, hardware resets the machine after 60s
```

This is the absolute last resort — a full system reboot.

---

## Level 1: Service Supervision (aios-init)

`aios-init` (PID 1) supervises all core services:

```rust
// initd/src/supervisor.rs

struct ServiceSupervisor {
    services: Vec<SupervisedService>,
}

struct SupervisedService {
    name: String,              // "aios-runtime"
    binary: String,            // "/usr/sbin/aios-runtime"
    pid: Option<u32>,
    restart_count: u32,
    last_restart: Option<Instant>,
    max_restarts: u32,         // 5
    restart_window: Duration,  // 5 minutes
    critical: bool,            // If true, system cannot operate without it
}

impl ServiceSupervisor {
    async fn monitor_loop(&mut self) {
        loop {
            for service in &mut self.services {
                if !service.is_running() {
                    if service.restart_count < service.max_restarts {
                        log::warn!("{} died, restarting ({}/{})",
                            service.name, service.restart_count + 1, service.max_restarts);
                        service.restart().await;
                    } else {
                        log::error!("{} exceeded max restarts", service.name);
                        if service.critical {
                            // Critical service can't restart — enter degraded mode
                            self.enter_degraded_mode(&service.name).await;
                        }
                    }
                }
            }
            // Reset restart counters if window has passed
            self.reset_stale_counters();
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn enter_degraded_mode(&self, failed_service: &str) {
        log::error!("DEGRADED MODE: {} unrecoverable", failed_service);
        // 1. Stop all non-essential agents
        // 2. Keep monitoring and networking alive
        // 3. Alert via management console
        // 4. Attempt repair every 60 seconds
        // 5. If runtime failed: system operates in reactive-only mode
        // 6. If orchestrator failed: agents continue last known tasks
    }
}
```

### Service Dependencies

```
aios-runtime     → CRITICAL (no AI without it)
aios-memory      → CRITICAL (no memory, agents can't function properly)
aios-tools       → CRITICAL (agents can't do anything without tools)
aios-orchestrator → IMPORTANT (agents continue with cached tasks)
agents           → RECOVERABLE (restart individually)
management       → NON-CRITICAL (system works without it)
```

---

## Level 2: Agent Supervision (Orchestrator)

The orchestrator monitors agent health:

```python
# In orchestrator

async def agent_health_loop():
    while True:
        for agent in registered_agents:
            try:
                health = await agent.health_check(timeout=5)
                if health.status != "healthy":
                    await handle_unhealthy_agent(agent, health)
            except TimeoutError:
                await handle_unresponsive_agent(agent)

        await asyncio.sleep(10)

async def handle_unresponsive_agent(agent):
    agent.missed_heartbeats += 1

    if agent.missed_heartbeats >= 3:
        # Agent is dead — kill and restart
        log.warn(f"Agent {agent.name} unresponsive, restarting")
        await kill_agent(agent)
        await spawn_agent(agent.name, agent.config)

        # Reassign any in-progress tasks
        for task in agent.active_tasks:
            await task_queue.requeue(task)

async def handle_unhealthy_agent(agent, health):
    if health.memory_usage > 0.9:
        # Agent is running out of memory — restart with clean state
        await restart_agent(agent)

    if health.error_rate > 0.5:
        # Agent is failing too many tasks — investigate
        analysis = await strategic_think(
            f"Agent {agent.name} has 50%+ error rate. "
            f"Recent errors: {health.recent_errors}. "
            f"Should I restart it, reconfigure it, or investigate further?"
        )
        await execute_recovery_plan(analysis)
```

---

## Level 3: Task-Level Error Handling

When an agent's task fails:

```python
# In orchestrator result_aggregator

async def handle_task_failure(task, error):
    # Strategy 1: Retry (for transient errors)
    if is_transient(error) and task.retry_count < 3:
        task.retry_count += 1
        await task_queue.requeue(task, delay=backoff(task.retry_count))
        return

    # Strategy 2: Reassign (agent might be broken)
    if task.reassign_count < 2:
        alternative_agent = find_alternative_agent(task)
        if alternative_agent:
            task.reassign_count += 1
            await assign_task(task, alternative_agent)
            return

    # Strategy 3: Escalate (ask smarter model)
    if task.intelligence_level != "strategic":
        escalated = task.clone()
        escalated.intelligence_level = "strategic"
        await task_queue.enqueue(escalated)
        return

    # Strategy 4: Fail gracefully
    log.error(f"Task {task.id} failed after all recovery attempts")
    await goal_engine.task_permanently_failed(task)
    # Goal engine decides if the goal can still be achieved
    # or if it should be marked as failed
```

### Transient Error Detection
```python
TRANSIENT_ERRORS = [
    "connection refused",      # Service temporarily down
    "timeout",                 # Network/service slow
    "resource temporarily unavailable",
    "too many open files",
    "no space left on device",  # Might be resolved by cleanup
    "rate limit exceeded",      # API throttling
]

def is_transient(error: str) -> bool:
    return any(pattern in error.lower() for pattern in TRANSIENT_ERRORS)
```

---

## Level 4: Tool-Level Recovery

Tools handle their own recoverable errors:

```rust
// In tool executor

async fn execute_with_recovery(tool: &Tool, input: &Value) -> Result<Value> {
    // Attempt 1: Normal execution
    match tool.execute(input).await {
        Ok(result) => return Ok(result),
        Err(e) if e.is_recoverable() => {
            // Attempt 2: Wait and retry
            tokio::time::sleep(Duration::from_secs(1)).await;
            match tool.execute(input).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Attempt 3: Rollback any partial state and report
                    if let Some(backup) = &tool.pre_execution_backup {
                        backup.restore().await?;
                    }
                    return Err(e);
                }
            }
        }
        Err(e) => return Err(e),
    }
}
```

---

## Degraded Operation Modes

| Mode | Trigger | Behavior |
|---|---|---|
| **Full Autonomy** | Normal operation | All systems operational |
| **Limited API** | API budget exhausted or offline | Local models only, complex tasks deferred |
| **Reduced Agents** | Some agents crashed, won't restart | Remaining agents cover critical functions |
| **Minimal** | Runtime or orchestrator failed | Basic system monitoring, no goal processing |
| **Safe Mode** | Multiple critical failures | Only init + networking + SSH, await human |
| **Recovery** | Booting after crash | Full system check, replay interrupted goals |

---

## Post-Crash Recovery

When aiOS boots after an unexpected shutdown:

```
1. aios-init detects: /var/lib/aios/clean_shutdown does NOT exist
2. Log: "Unclean shutdown detected — entering recovery"
3. Check filesystem integrity (fsck)
4. Start services normally
5. Once orchestrator is up:
   a. Load interrupted goals from working memory
   b. Check: which tasks were in-progress at crash time?
   c. For each interrupted task:
      - If idempotent: re-execute
      - If not idempotent: check current state, determine if it completed
      - If state is inconsistent: call rollback, then re-execute
   d. Resume normal autonomy loop
6. Create /var/lib/aios/clean_shutdown (deleted at shutdown)
7. Log incident report to long-term memory
```
