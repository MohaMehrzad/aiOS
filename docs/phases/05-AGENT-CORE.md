# Phase 5: Agent Core Framework

## Goal
Build the orchestrator, agent runtime, gRPC communication layer, task planning, and intelligence routing. This is the BRAIN of aiOS — the most complex phase.

## Prerequisites
- Phase 4 complete (local AI runtime operational)
- Read [architecture/AGENT-FRAMEWORK.md](../architecture/AGENT-FRAMEWORK.md) — entire document
- Read [architecture/SYSTEM.md](../architecture/SYSTEM.md) — orchestrator and IPC sections

---

## Step-by-Step

### Step 5.1: Define gRPC Proto Files

**Claude Code prompt**: "Create all gRPC proto definitions for orchestrator-agent communication, task management, and agent registration"

```
Files to create:
  agent-core/proto/orchestrator.proto  — Goal/task management, agent registration
  agent-core/proto/agent.proto         — Agent interface (what agents implement)
  agent-core/proto/common.proto        — Shared message types
  agent-core/proto/runtime.proto       — AI runtime inference API (from Phase 4)
```

```protobuf
// agent-core/proto/common.proto
syntax = "proto3";
package aios.common;

message Empty {}

message Status {
    bool success = 1;
    string message = 2;
}

message AgentId {
    string id = 1;
}

message GoalId {
    string id = 1;
}

message Goal {
    string id = 1;
    string description = 2;
    int32 priority = 3;              // 0=critical, 1=high, 2=normal, 3=low
    string submitted_by = 4;
    int64 submitted_at = 5;          // Unix timestamp
    map<string, string> metadata = 6;
}

message GoalStatus {
    string goal_id = 1;
    string status = 2;               // pending, planning, active, completed, failed
    float progress = 3;              // 0.0 to 1.0
    int32 tasks_total = 4;
    int32 tasks_completed = 5;
    string result = 6;
    repeated Task tasks = 7;
}

message GoalFilter {
    string status = 1;               // Filter by status, empty = all
    int32 limit = 2;
}

message GoalList {
    repeated GoalStatus goals = 1;
}

message Task {
    string id = 1;
    string goal_id = 2;
    string description = 3;
    string assigned_agent = 4;
    string status = 5;               // pending, assigned, running, completed, failed
    string intelligence_level = 6;   // reactive, operational, tactical, strategic
    bytes input_json = 7;
    bytes output_json = 8;
    repeated string depends_on = 9;  // Task IDs this depends on
    int64 created_at = 10;
    int64 started_at = 11;
    int64 completed_at = 12;
}

message TaskRequest {
    string agent_id = 1;
}

message TaskResult {
    string task_id = 1;
    bool success = 2;
    bytes output_json = 3;
    string error = 4;
    int64 duration_ms = 5;
}

message SubTaskRequest {
    string parent_task_id = 1;
    string description = 2;
    string target_agent = 3;
}

message AgentRegistration {
    string name = 1;
    string agent_type = 2;
    repeated string capabilities = 3;
    repeated string tools = 4;
    int32 port = 5;                  // Agent's gRPC port
}

message RegistrationResponse {
    bool accepted = 1;
    string agent_id = 2;
    repeated string granted_capabilities = 3;
}

message AgentHeartbeat {
    string agent_id = 1;
    string status = 2;               // healthy, busy, degraded
    int32 active_tasks = 3;
    float cpu_percent = 4;
    float memory_mb = 5;
}

message HeartbeatResponse {
    bool acknowledged = 1;
    repeated string pending_commands = 2; // shutdown, reconfigure, etc.
}

message AgentHealth {
    string status = 1;
    float cpu_percent = 2;
    float memory_mb = 3;
    int32 active_tasks = 4;
    float error_rate = 5;
    repeated string recent_errors = 6;
}

message InferenceRequest {
    string prompt = 1;
    string system_prompt = 2;
    int32 max_tokens = 3;
    float temperature = 4;
    string intelligence_level = 5;
    string requesting_agent = 6;
    string task_id = 7;
}

message InferenceResponse {
    string text = 1;
    string model_used = 2;
    int32 tokens_used = 3;
    int64 latency_ms = 4;
    float cost_usd = 5;
}

message ServiceRegistration {
    string name = 1;
    string address = 2;
    int32 port = 3;
    string health_check_path = 4;
    string managed_by = 5;
}

message ServiceQuery {
    string name = 1;
}

message ServiceInfo {
    string name = 1;
    string address = 2;
    int32 port = 3;
    string status = 4;
    string managed_by = 5;
}

message ShutdownRequest {
    int32 timeout_seconds = 1;
    string reason = 2;
}

message ProgressUpdate {
    string task_id = 1;
    float progress = 2;
    string message = 3;
}
```

```protobuf
// agent-core/proto/orchestrator.proto
syntax = "proto3";
package aios.orchestrator;

import "common.proto";

service Orchestrator {
    // Agent registration
    rpc RegisterAgent(AgentRegistration) returns (RegistrationResponse);
    rpc DeregisterAgent(AgentId) returns (Status);
    rpc Heartbeat(AgentHeartbeat) returns (HeartbeatResponse);

    // Goal management (from management console)
    rpc SubmitGoal(Goal) returns (GoalStatus);
    rpc GetGoalStatus(GoalId) returns (GoalStatus);
    rpc CancelGoal(GoalId) returns (Status);
    rpc ListGoals(GoalFilter) returns (GoalList);

    // Task management (agent-facing)
    rpc GetTask(TaskRequest) returns (Task);
    rpc ReportTaskResult(TaskResult) returns (Status);
    rpc RequestSubTask(SubTaskRequest) returns (Task);

    // Intelligence routing
    rpc RequestInference(InferenceRequest) returns (InferenceResponse);

    // Service discovery
    rpc RegisterService(ServiceRegistration) returns (Status);
    rpc FindService(ServiceQuery) returns (ServiceInfo);
}

// agent-core/proto/agent.proto
syntax = "proto3";
package aios.agent;

import "common.proto";

service Agent {
    // Called by orchestrator to assign a task
    rpc ExecuteTask(Task) returns (TaskResult);

    // Called by orchestrator to check agent health
    rpc HealthCheck(Empty) returns (AgentHealth);

    // Called by orchestrator for graceful shutdown
    rpc Shutdown(ShutdownRequest) returns (Status);

    // Stream: agent sends progress updates
    rpc StreamProgress(stream ProgressUpdate) returns (Status);
}
```

### Step 5.2: Implement the Orchestrator (Rust)

**Claude Code prompt**: "Implement the aios-orchestrator Rust service with goal engine, task planner, agent router, and result aggregator"

```
File: agent-core/src/main.rs + submodules

Modules:
  agent-core/src/
  ├── main.rs              — Service startup, gRPC server
  ├── goal_engine.rs       — Goal queue, prioritization, lifecycle
  ├── task_planner.rs      — Goal → task DAG decomposition
  ├── agent_router.rs      — Task → agent assignment
  ├── result_aggregator.rs — Collect results, determine goal completion
  ├── decision_logger.rs   — Log all decisions with reasoning
  ├── agent_registry.rs    — Track registered agents and their capabilities
  ├── service_registry.rs  — Track registered services
  ├── intelligence.rs      — Intelligence level routing logic
  └── autonomy_loop.rs     — Main autonomy loop
```

#### Goal Engine
```rust
struct GoalEngine {
    goals: PriorityQueue<Goal>,
    active_goals: HashMap<GoalId, ActiveGoal>,
}

impl GoalEngine {
    fn enqueue(&mut self, goal: Goal) {
        // Assign priority:
        //   Critical (system health): 0
        //   High (security): 1
        //   Normal (user request): 2
        //   Low (optimization): 3
    }

    fn next(&mut self) -> Option<Goal> {
        // Return highest priority goal that isn't blocked
    }

    fn update(&mut self, goal_id: GoalId, result: TaskResult) {
        // Update goal progress based on task result
        // If all tasks complete → mark goal complete
        // If task failed → determine retry/escalate/fail
    }
}
```

#### Task Planner
```rust
struct TaskPlanner {
    runtime_client: AIRuntimeClient,  // For local model inference
    api_client: Option<ApiGatewayClient>,  // For Claude API (if available)
    pattern_cache: HashMap<String, TaskDAG>,  // Cached plans
}

impl TaskPlanner {
    async fn plan(&self, goal: &Goal) -> Result<TaskDAG> {
        // 1. Check pattern cache for similar goals
        if let Some(cached) = self.find_cached_plan(goal) {
            return Ok(cached);
        }

        // 2. Try local model first (tactical layer)
        let plan = self.plan_with_local_model(goal).await;
        if plan.is_confident() {
            return Ok(plan);
        }

        // 3. Escalate to Claude API (strategic layer)
        let plan = self.plan_with_api(goal).await?;

        // 4. Cache the plan pattern for future use
        self.cache_plan(goal, &plan);

        Ok(plan)
    }
}
```

### Step 5.3: Implement Agent Runtime (Python)

**Claude Code prompt**: "Implement the Python agent runtime — the base class all agents inherit from, with gRPC client, tool calling, and lifecycle management"

```python
# agent-core/python/aios_agent/base.py

class BaseAgent:
    """Base class for all aiOS agents."""

    def __init__(self, name: str, config: AgentConfig):
        self.name = name
        self.config = config
        self.orchestrator = OrchestratorClient()
        self.tool_client = ToolClient()
        self.runtime_client = RuntimeClient()
        self.working_memory: dict = {}

    async def start(self):
        """Register with orchestrator and enter main loop."""
        await self.orchestrator.register(
            name=self.name,
            capabilities=self.get_capabilities(),
            tools=self.get_tools(),
        )
        await self.main_loop()

    async def main_loop(self):
        """Wait for tasks and execute them."""
        while True:
            task = await self.orchestrator.get_task(agent=self.name)
            if task:
                try:
                    result = await self.execute_task(task)
                    await self.orchestrator.report_result(task.id, result)
                except Exception as e:
                    await self.orchestrator.report_error(task.id, str(e))

            await self.orchestrator.heartbeat(self.name)
            await asyncio.sleep(0.1)

    async def execute_task(self, task: Task) -> TaskResult:
        """Override in subclass to handle specific task types."""
        raise NotImplementedError

    async def call_tool(self, tool_name: str, **kwargs) -> ToolResult:
        """Call a tool through the tool registry."""
        return await self.tool_client.execute(
            tool_name=tool_name,
            agent_id=self.name,
            task_id=self.current_task.id,
            input=kwargs,
            reason=f"Agent {self.name} executing {tool_name} for task {self.current_task.id}"
        )

    async def think(self, prompt: str, level: str = "auto") -> str:
        """Use AI inference for decision making."""
        response = await self.runtime_client.infer(
            prompt=prompt,
            system_prompt=self.system_prompt,
            intelligence_level=level,
        )
        return response.text

    # Subclass interface
    def get_capabilities(self) -> list[str]:
        """Return list of capabilities this agent provides."""
        raise NotImplementedError

    def get_tools(self) -> list[str]:
        """Return list of tools this agent uses."""
        raise NotImplementedError

    @property
    def system_prompt(self) -> str:
        """Return the system prompt for this agent."""
        raise NotImplementedError
```

### Step 5.4: Implement System Agent

**Claude Code prompt**: "Implement the System Agent — handles file operations, process management, and service lifecycle"

```python
# agent-core/python/aios_agent/agents/system.py

class SystemAgent(BaseAgent):
    """Manages files, processes, and services."""

    @property
    def system_prompt(self) -> str:
        return """You are the System Agent for aiOS. You manage files, processes, and services.
        You have access to fs.*, process.*, service.*, and config.* tools.
        Always verify before modifying. Never delete without confirmation from orchestrator.
        Log every action with a reason. Prefer the simplest solution that works."""

    def get_capabilities(self) -> list[str]:
        return ["file_management", "process_management", "service_management", "configuration"]

    def get_tools(self) -> list[str]:
        return ["fs.*", "process.*", "service.*", "config.*"]

    async def execute_task(self, task: Task) -> TaskResult:
        # Use AI to determine the right tool calls for this task
        plan = await self.think(
            f"Given this task: {task.description}\n"
            f"What tool calls should I make? Available tools: {self.get_tools()}\n"
            f"Respond with a JSON list of tool calls.",
            level="tactical"
        )

        tool_calls = parse_tool_calls(plan)
        results = []
        for call in tool_calls:
            result = await self.call_tool(call.tool, **call.args)
            results.append(result)
            if not result.success:
                return TaskResult(success=False, error=result.error)

        return TaskResult(success=True, data=results)
```

### Step 5.5: Implement Remaining Agents (Stubs)

**Claude Code prompt**: "Create stub implementations for all remaining agents — network, security, monitor, package, storage, task, dev — following the same pattern as System Agent"

Each agent gets:
1. A system prompt defining its role
2. A capabilities list
3. A tools list
4. An `execute_task` method (initially delegates to AI for tool call planning)

### Step 5.6: Implement Intelligence Routing

**Claude Code prompt**: "Implement the intelligence routing logic that classifies tasks and routes to the appropriate model tier"

```rust
// agent-core/src/intelligence.rs

enum IntelligenceLevel {
    Reactive,      // Heuristics, no AI
    Operational,   // TinyLlama 1.1B
    Tactical,      // Mistral 7B / Phi-3
    Strategic,     // Claude API / OpenAI API
}

fn classify_task(task: &Task) -> IntelligenceLevel {
    // Rule-based classification
    let desc = task.description.to_lowercase();

    // Reactive: exact matches to known patterns
    if task.matches_cached_pattern() {
        return IntelligenceLevel::Reactive;
    }

    // Operational: simple checks and classifications
    if desc.contains("check if") || desc.contains("list") || desc.contains("status") {
        return IntelligenceLevel::Operational;
    }

    // Strategic: complex reasoning required
    if desc.contains("plan") || desc.contains("design") || desc.contains("debug")
        || desc.contains("security") || desc.contains("analyze") {
        return IntelligenceLevel::Strategic;
    }

    // Default: tactical
    IntelligenceLevel::Tactical
}
```

### Step 5.7: Implement Autonomy Loop

**Claude Code prompt**: "Implement the main autonomy loop in the orchestrator that continuously processes goals, runs health checks, and responds to triggers"

See [architecture/AGENT-FRAMEWORK.md](../architecture/AGENT-FRAMEWORK.md) — Autonomy Loop section.

### Step 5.8: Integration Test

**Claude Code prompt**: "Create an integration test: submit a goal, verify it decomposes into tasks, tasks route to the right agent, agent calls tools, and result is reported back"

Test scenario: "Check system disk usage and report"
1. Submit goal via gRPC
2. Orchestrator decomposes into tasks: [call monitor.disk, analyze result, report]
3. Tasks route to Monitor Agent
4. Monitor Agent calls `monitor.disk` tool
5. Agent reports disk usage back
6. Goal marked complete

---

## Deliverables Checklist

- [ ] All gRPC proto files defined and compiling
- [ ] Orchestrator compiles and starts (goal engine, task planner, agent router)
- [ ] Python agent runtime (BaseAgent) works
- [ ] System Agent can execute simple tasks
- [ ] All 8 agent stubs created
- [ ] Intelligence routing classifies tasks correctly
- [ ] Autonomy loop runs continuously
- [ ] Agent registration and heartbeat work
- [ ] End-to-end test: goal → tasks → agent → tools → result
- [ ] aios-init starts orchestrator after runtime

---

## Next Phase
Once integration test passes → [Phase 6: Tool Registry](./06-TOOLS.md)
