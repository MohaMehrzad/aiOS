# Agent Framework Architecture

## Overview

The agent framework is the brain of aiOS. It consists of an **orchestrator** that decomposes goals into tasks, and a **mesh of specialized agents** that execute those tasks using tools.

---

## Core Concepts

### Goal
A high-level objective from a human or another system. Examples:
- "Keep this server secure and up to date"
- "Deploy this Python application"
- "Optimize disk usage"

### Task
A concrete, atomic action that an agent can execute. Goals decompose into tasks. Examples:
- "Check available disk space on /var"
- "Install package nginx version 1.24"
- "Write file /etc/nginx/nginx.conf with content X"

### Agent
A specialized AI process that handles a domain of tasks. Each agent:
- Has a system prompt defining its role and capabilities
- Has access to a subset of tools relevant to its domain
- Can use local models for simple decisions or escalate to API for complex ones
- Maintains its own working memory
- Reports results back to the orchestrator

### Tool
A structured function that performs a system operation. Tools are the ONLY way agents interact with the system. See [TOOL-REGISTRY.md](./TOOL-REGISTRY.md).

---

## Orchestrator Design

The orchestrator is the central coordinator. It is NOT an agent — it's the conductor.

### Components

```
┌─────────────────────────────────────────────────────────┐
│                     ORCHESTRATOR                         │
│                                                          │
│  ┌──────────────┐   ┌──────────────┐   ┌─────────────┐ │
│  │  Goal Engine  │──>│ Task Planner │──>│ Agent Router│ │
│  └──────────────┘   └──────────────┘   └──────┬──────┘ │
│         ▲                                       │        │
│         │            ┌──────────────┐           │        │
│         └────────────│  Result      │<──────────┘        │
│                      │  Aggregator  │                    │
│                      └──────┬───────┘                    │
│                             │                            │
│                      ┌──────▼───────┐                    │
│                      │  Decision    │                    │
│                      │  Logger      │                    │
│                      └──────────────┘                    │
└─────────────────────────────────────────────────────────┘
```

### Goal Engine
- Receives goals from management console or internal triggers
- Validates goals against security policies
- Prioritizes goals (critical system health > user requests > optimization)
- Maintains goal queue with dependencies

### Task Planner
- Decomposes goals into ordered task lists
- Uses Claude API for complex planning (multi-step goals)
- Uses local models for simple/known patterns (cached plans)
- Outputs a DAG (Directed Acyclic Graph) of tasks with dependencies

```python
# Example task DAG for "Deploy Python app"
TaskDAG:
  task_1: {action: "check_requirements", agent: "system", deps: []}
  task_2: {action: "install_python_deps", agent: "package", deps: [task_1]}
  task_3: {action: "create_virtualenv", agent: "system", deps: [task_2]}
  task_4: {action: "copy_app_files", agent: "system", deps: [task_3]}
  task_5: {action: "configure_service", agent: "system", deps: [task_4]}
  task_6: {action: "start_service", agent: "system", deps: [task_5]}
  task_7: {action: "verify_health", agent: "monitor", deps: [task_6]}
  task_8: {action: "configure_firewall", agent: "network", deps: [task_6]}
```

### Agent Router
- Maps tasks to the best available agent
- Load balances across agent instances
- Handles agent failures (retry, reassign, escalate)
- Tracks agent health and availability

### Result Aggregator
- Collects task results from agents
- Determines if goal is achieved
- Handles partial failures (retry failed tasks, compensating actions)
- Reports final status back to Goal Engine

### Decision Logger
- Records every decision with full context
- WHY was this plan chosen?
- WHY was this agent selected?
- WHAT was the result?
- Stores in memory system for future learning

---

## Agent Types

### System Agent (`aios-agent-system`)
**Domain**: File operations, process management, service lifecycle, system configuration

**Tools**: `fs.*`, `process.*`, `service.*`, `config.*`

**Local model tasks**: File existence checks, simple config parsing, process status interpretation

**API escalation**: Complex configuration generation, troubleshooting cascading failures

**System prompt core**:
```
You are the System Agent for aiOS. You manage files, processes, and services.
You have access to fs.*, process.*, service.*, and config.* tools.
Always verify before modifying. Never delete without confirmation from orchestrator.
Log every action with a reason.
```

### Network Agent (`aios-agent-network`)
**Domain**: Network interfaces, firewall, DNS, routing, VPN, HTTP

**Tools**: `net.*`, `firewall.*`, `dns.*`

**Local model tasks**: Parse IP addresses, check port availability, simple firewall rules

**API escalation**: Complex network topology decisions, security policy evaluation

### Security Agent (`aios-agent-security`)
**Domain**: Access control, vulnerability scanning, intrusion detection, certificate management

**Tools**: `sec.*`, `audit.*`, `crypto.*`

**Local model tasks**: Log anomaly detection, known CVE pattern matching

**API escalation**: Threat analysis, policy creation, incident response planning

### Monitor Agent (`aios-agent-monitor`)
**Domain**: System metrics, health checks, alerting, performance monitoring

**Tools**: `monitor.*`, `metrics.*`, `alert.*`

**Local model tasks**: Metric threshold analysis, trend detection, simple alerting

**API escalation**: Root cause analysis, capacity planning, anomaly investigation

### Package Agent (`aios-agent-package`)
**Domain**: Package installation, updates, dependency resolution, vulnerability tracking

**Tools**: `pkg.*`

**Local model tasks**: Version comparison, simple dependency checks

**API escalation**: Complex dependency resolution, security impact assessment

### Storage Agent (`aios-agent-storage`)
**Domain**: Disk management, backups, data lifecycle, cleanup

**Tools**: `storage.*`, `backup.*`

**Local model tasks**: Disk space analysis, identifying large/old files

**API escalation**: Backup strategy decisions, data migration planning

### Task Agent (`aios-agent-task`)
**Domain**: General-purpose task execution, scripting, automation

**Tools**: All tools (restricted by orchestrator per-task)

**Local model tasks**: Simple script generation, command construction

**API escalation**: Complex automation workflows, multi-step scripting

### Dev Agent (`aios-agent-dev`)
**Domain**: Software development, code writing, testing, deployment

**Tools**: `fs.*`, `process.*`, `git.*`, `build.*`, `test.*`

**Local model tasks**: Simple code formatting, syntax checking

**API escalation**: Code generation, architecture decisions, debugging

---

## Agent Lifecycle

```
1. SPAWN
   - Orchestrator starts agent process
   - Agent loads its system prompt and configuration
   - Agent connects to orchestrator via gRPC
   - Agent registers its capabilities

2. IDLE
   - Agent waits for task assignments
   - Maintains heartbeat with orchestrator (every 5s)
   - Local model stays warm for fast response

3. ACTIVE
   - Receives task from orchestrator
   - Determines if local model or API is needed
   - Executes tool calls
   - Reports progress back to orchestrator
   - Logs all actions to memory

4. COMPLETE
   - Returns result to orchestrator
   - Updates its working memory
   - Returns to IDLE state

5. ERROR
   - Task failed — reports error with context
   - Orchestrator decides: retry, reassign, or escalate
   - Agent returns to IDLE or is restarted if unhealthy

6. SHUTDOWN
   - Graceful shutdown signal from orchestrator
   - Agent completes current task (with timeout)
   - Saves working memory to persistent storage
   - Deregisters from orchestrator
   - Process exits
```

---

## Intelligence Routing

The critical question for every task: **which brain should handle this?**

```python
def route_intelligence(task: Task) -> IntelligenceLevel:
    # Level 1: Reactive (heuristics, no AI)
    if task.matches_known_pattern() and task.is_simple():
        return IntelligenceLevel.REACTIVE

    # Level 2: Operational (tiny local model, <1B params)
    if task.is_classification() or task.is_simple_analysis():
        return IntelligenceLevel.OPERATIONAL

    # Level 3: Tactical (local 7B model)
    if task.needs_reasoning() and not task.is_novel():
        return IntelligenceLevel.TACTICAL

    # Level 4: Strategic (Claude/GPT API)
    if task.is_novel() or task.is_complex() or task.is_security_critical():
        return IntelligenceLevel.STRATEGIC

    # Default: tactical
    return IntelligenceLevel.TACTICAL
```

### Routing Heuristics

| Signal | Routes To |
|---|---|
| Task matches cached pattern exactly | REACTIVE |
| Task is "check if X exists" | OPERATIONAL |
| Task is "parse this log entry" | OPERATIONAL |
| Task is "decide best approach for X" | TACTICAL |
| Task is "generate configuration for X" | TACTICAL or STRATEGIC |
| Task is "plan how to achieve X" | STRATEGIC |
| Task is "write code for X" | STRATEGIC |
| Task involves security decisions | STRATEGIC |
| Task is novel (never seen before) | STRATEGIC |

### Cost Tracking
Every API call is tracked:
```
{
  "task_id": "task-123",
  "model": "claude-sonnet-4-5-20250929",
  "input_tokens": 1500,
  "output_tokens": 800,
  "cost_usd": 0.0078,
  "latency_ms": 2300,
  "could_have_been_local": false
}
```

Monthly budget enforcement — if approaching limit, more aggressively route to local models.

---

## Agent-to-Agent Communication

Agents can request help from other agents through the orchestrator:

```
System Agent: "I need to configure nginx, but I need to know which ports are available"
  → Orchestrator routes sub-task to Network Agent
  → Network Agent: "Ports 80, 443, 8080 are free"
  → System Agent receives answer, continues its task
```

This is always mediated by the orchestrator — agents never talk directly to each other. This ensures:
1. All communication is logged
2. The orchestrator maintains global state awareness
3. No circular dependencies between agents

---

## Autonomy Loop

The orchestrator runs a continuous loop:

```python
async def autonomy_loop():
    while True:
        # 1. Check for new goals from management console
        new_goals = await management_api.poll_goals()
        for goal in new_goals:
            goal_engine.enqueue(goal)

        # 2. Check for internal triggers
        triggers = await monitor_agent.get_triggers()
        for trigger in triggers:
            goal = trigger.to_goal()
            goal_engine.enqueue(goal)

        # 3. Process goal queue
        while goal_engine.has_pending():
            goal = goal_engine.next()
            tasks = await task_planner.plan(goal)
            for task in tasks.ready():
                agent = agent_router.assign(task)
                await agent.execute(task)

        # 4. Collect results
        results = await result_aggregator.collect()
        for result in results:
            decision_logger.log(result)
            goal_engine.update(result)

        # 5. Proactive checks (every cycle)
        await self.run_health_checks()
        await self.check_resource_usage()
        await self.review_security_posture()

        # 6. Brief pause
        await asyncio.sleep(1)  # 1 second cycle
```

---

## Implementation Details

### Language Split
- **Orchestrator**: Rust (performance, reliability, no GC pauses)
- **Agent runtime**: Python (faster iteration, rich AI library ecosystem)
- **Communication**: gRPC with protobuf (language-agnostic bridge)

### Agent Process Model
Each agent runs as a separate OS process:
- Crash isolation — one agent crash doesn't take down others
- Independent resource limits (cgroups)
- Can be restarted independently
- Python agents use `asyncio` for concurrent tool calls

### Scaling
- Default: 1 instance per agent type
- Under load: orchestrator can spawn additional instances
- Example: 3x Task Agents during heavy workload
- Agent pool managed by orchestrator, max instances configured per type
