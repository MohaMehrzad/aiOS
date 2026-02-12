# Complete gRPC Proto Definitions

## Overview

All inter-service communication in aiOS uses gRPC with Protocol Buffers. This document contains the complete proto definitions for every service.

Proto files live in `agent-core/proto/` and are compiled for both Rust (tonic/prost) and Python (grpcio-tools).

---

## File Index

| Proto File | Package | Service | Used By |
|---|---|---|---|
| `common.proto` | `aios.common` | (shared types) | Everything |
| `orchestrator.proto` | `aios.orchestrator` | `Orchestrator` | Agents, management console |
| `agent.proto` | `aios.agent` | `Agent` | Orchestrator → agents |
| `runtime.proto` | `aios.runtime` | `AIRuntime` | Orchestrator, agents |
| `tools.proto` | `aios.tools` | `ToolRegistry` | Agents |
| `memory.proto` | `aios.memory` | `MemoryService` | Orchestrator, agents |
| `api_gateway.proto` | `aios.api_gateway` | `ApiGateway` | Orchestrator |

---

## common.proto

Defined in [Phase 5 doc](../phases/05-AGENT-CORE.md) — contains all shared message types (Empty, Status, Goal, Task, AgentRegistration, InferenceRequest, etc.).

---

## tools.proto

```protobuf
// agent-core/proto/tools.proto
syntax = "proto3";
package aios.tools;

service ToolRegistry {
    // Discovery
    rpc ListTools(ListToolsRequest) returns (ListToolsResponse);
    rpc GetTool(GetToolRequest) returns (ToolDefinition);

    // Execution
    rpc Execute(ExecuteRequest) returns (ExecuteResponse);
    rpc Rollback(RollbackRequest) returns (RollbackResponse);

    // Extension
    rpc Register(RegisterToolRequest) returns (RegisterToolResponse);
    rpc Deregister(DeregisterToolRequest) returns (Status);
}

// --- Discovery ---

message ListToolsRequest {
    string namespace = 1;        // Filter by namespace (e.g., "fs"), empty = all
}

message ListToolsResponse {
    repeated ToolDefinition tools = 1;
}

message GetToolRequest {
    string name = 1;             // Full tool name (e.g., "fs.read")
}

message ToolDefinition {
    string name = 1;             // "fs.read"
    string namespace = 2;        // "fs"
    string version = 3;          // "1.0.0"
    string description = 4;      // Human-readable description
    bytes input_schema = 5;      // JSON Schema for input validation
    bytes output_schema = 6;     // JSON Schema for output validation
    repeated string required_capabilities = 7;  // e.g., ["fs:read"]
    string risk_level = 8;       // "low", "medium", "high", "critical"
    bool requires_confirmation = 9;
    bool idempotent = 10;
    bool reversible = 11;
    int32 timeout_ms = 12;
    string rollback_tool = 13;   // Tool to call for rollback, empty if not reversible
}

// --- Execution ---

message ExecuteRequest {
    string tool_name = 1;        // "fs.write"
    string agent_id = 2;         // Calling agent's ID
    string task_id = 3;          // Parent task ID
    bytes input_json = 4;        // JSON input matching tool's input_schema
    string reason = 5;           // Why the agent is calling this tool
}

message ExecuteResponse {
    bool success = 1;
    bytes output_json = 2;       // JSON output matching tool's output_schema
    string error = 3;            // Error message if !success
    string execution_id = 4;     // Unique ID for this execution (for rollback)
    int64 duration_ms = 5;
    string backup_id = 6;        // Backup reference if tool is reversible
}

message RollbackRequest {
    string execution_id = 1;     // From ExecuteResponse
    string reason = 2;
}

message RollbackResponse {
    bool success = 1;
    string error = 2;
}

// --- Extension ---

message RegisterToolRequest {
    ToolDefinition tool = 1;
    string handler_address = 2;  // gRPC address of custom tool handler
}

message RegisterToolResponse {
    bool accepted = 1;
    string error = 2;
}

message DeregisterToolRequest {
    string tool_name = 1;
}

// Shared
message Status {
    bool success = 1;
    string message = 2;
}
```

---

## memory.proto

```protobuf
// agent-core/proto/memory.proto
syntax = "proto3";
package aios.memory;

service MemoryService {
    // --- Operational Memory (hot, in-memory) ---
    rpc PushEvent(Event) returns (Empty);
    rpc GetRecentEvents(RecentEventsRequest) returns (EventList);
    rpc UpdateMetric(MetricUpdate) returns (Empty);
    rpc GetMetric(MetricRequest) returns (MetricValue);
    rpc GetSystemSnapshot(Empty) returns (SystemSnapshot);

    // --- Working Memory (warm, SQLite) ---
    rpc StoreGoal(GoalRecord) returns (Empty);
    rpc UpdateGoal(GoalUpdate) returns (Empty);
    rpc GetActiveGoals(Empty) returns (GoalList);

    rpc StoreTask(TaskRecord) returns (Empty);
    rpc GetTasksForGoal(GoalIdRequest) returns (TaskList);

    rpc StoreToolCall(ToolCallRecord) returns (Empty);
    rpc StoreDecision(Decision) returns (Empty);

    rpc StorePattern(Pattern) returns (Empty);
    rpc FindPattern(PatternQuery) returns (PatternResult);
    rpc UpdatePatternStats(PatternStatsUpdate) returns (Empty);

    rpc StoreAgentState(AgentState) returns (Empty);
    rpc GetAgentState(AgentStateRequest) returns (AgentState);

    // --- Long-Term Memory (cold, SQLite + ChromaDB) ---
    rpc SemanticSearch(SemanticSearchRequest) returns (SearchResults);
    rpc StoreProcedure(Procedure) returns (Empty);
    rpc StoreIncident(Incident) returns (Empty);
    rpc StoreConfigChange(ConfigChange) returns (Empty);

    // --- Knowledge Base ---
    rpc SearchKnowledge(SemanticSearchRequest) returns (SearchResults);
    rpc AddKnowledge(KnowledgeEntry) returns (Empty);

    // --- Context Assembly ---
    rpc AssembleContext(ContextRequest) returns (ContextResponse);
}

// --- Shared ---
message Empty {}

// --- Operational ---

message Event {
    string id = 1;
    int64 timestamp = 2;
    string category = 3;         // "metric", "event", "task", "tool_result", "error"
    string source = 4;           // Agent or service name
    bytes data_json = 5;
    bool critical = 6;           // If true, flush to working memory immediately
}

message RecentEventsRequest {
    int32 count = 1;
    string category = 2;         // Filter by category, empty = all
    string source = 3;           // Filter by source, empty = all
}

message EventList {
    repeated Event events = 1;
}

message MetricUpdate {
    string key = 1;              // "cpu.usage", "memory.used_mb", etc.
    double value = 2;
    int64 timestamp = 3;
}

message MetricRequest {
    string key = 1;
}

message MetricValue {
    string key = 1;
    double value = 2;
    int64 timestamp = 3;
}

message SystemSnapshot {
    double cpu_percent = 1;
    double memory_used_mb = 2;
    double memory_total_mb = 3;
    double disk_used_gb = 4;
    double disk_total_gb = 5;
    double gpu_utilization = 6;
    int32 active_tasks = 7;
    int32 active_agents = 8;
    repeated string loaded_models = 9;
}

// --- Working ---

message GoalRecord {
    string id = 1;
    string description = 2;
    string status = 3;
    int32 priority = 4;
    int64 created_at = 5;
    int64 completed_at = 6;
    string result = 7;
    bytes metadata_json = 8;
}

message GoalUpdate {
    string id = 1;
    string status = 2;
    string result = 3;
}

message GoalIdRequest {
    string goal_id = 1;
}

message GoalList {
    repeated GoalRecord goals = 1;
}

message TaskRecord {
    string id = 1;
    string goal_id = 2;
    string description = 3;
    string agent = 4;
    string status = 5;
    bytes input_json = 6;
    bytes output_json = 7;
    int64 started_at = 8;
    int64 completed_at = 9;
    int64 duration_ms = 10;
    string error = 11;
}

message TaskList {
    repeated TaskRecord tasks = 1;
}

message ToolCallRecord {
    string id = 1;
    string task_id = 2;
    string tool_name = 3;
    string agent = 4;
    bytes input_json = 5;
    bytes output_json = 6;
    bool success = 7;
    int64 duration_ms = 8;
    string reason = 9;
    int64 timestamp = 10;
}

message Decision {
    string id = 1;
    string context = 2;          // What situation triggered the decision
    bytes options_json = 3;      // What options were considered
    string chosen = 4;           // What was decided
    string reasoning = 5;       // WHY
    string intelligence_level = 6;
    string model_used = 7;
    string outcome = 8;          // Updated later: did it work?
    int64 timestamp = 9;
}

message Pattern {
    string id = 1;
    string trigger = 2;          // What situation triggers this pattern
    string action = 3;           // What to do
    double success_rate = 4;
    int32 uses = 5;
    int64 last_used = 6;
    string created_from = 7;     // Goal/task that first created this
}

message PatternQuery {
    string trigger = 1;
    double min_success_rate = 2;
}

message PatternResult {
    Pattern pattern = 1;
    bool found = 2;
}

message PatternStatsUpdate {
    string id = 1;
    bool success = 2;
}

message AgentState {
    string agent_name = 1;
    bytes state_json = 2;
    int64 updated_at = 3;
}

message AgentStateRequest {
    string agent_name = 1;
}

// --- Long-Term ---

message SemanticSearchRequest {
    string query = 1;
    repeated string collections = 2; // "decisions", "incidents", "procedures", "knowledge"
    int32 n_results = 3;
    double min_relevance = 4;
}

message SearchResult {
    string content = 1;
    bytes metadata_json = 2;
    double relevance = 3;
    string collection = 4;
    string id = 5;
}

message SearchResults {
    repeated SearchResult results = 1;
}

message Procedure {
    string id = 1;
    string name = 2;
    string description = 3;
    bytes steps_json = 4;        // Ordered list of tool calls
    int32 success_count = 5;
    int32 fail_count = 6;
    int64 avg_duration_ms = 7;
    repeated string tags = 8;
    int64 created_at = 9;
    int64 last_used = 10;
}

message Incident {
    string id = 1;
    string description = 2;
    bytes symptoms_json = 3;
    string root_cause = 4;
    string resolution = 5;
    string resolved_by = 6;
    string prevention = 7;
    int64 timestamp = 8;
}

message ConfigChange {
    string id = 1;
    string file_path = 2;
    string content = 3;
    string changed_by = 4;
    string reason = 5;
    int64 timestamp = 6;
}

message KnowledgeEntry {
    string title = 1;
    string content = 2;
    string source = 3;           // "man page", "package docs", "learned", etc.
    repeated string tags = 4;
}

// --- Context Assembly ---

message ContextRequest {
    string task_description = 1;
    int32 max_tokens = 2;
    repeated string memory_tiers = 3;  // "operational", "working", "longterm", "knowledge"
}

message ContextChunk {
    string source = 1;           // Which memory tier
    string content = 2;
    double relevance = 3;
    int32 tokens = 4;
}

message ContextResponse {
    repeated ContextChunk chunks = 1;
    int32 total_tokens = 2;
}
```

---

## runtime.proto

Defined in [Phase 4 doc](../phases/04-AI-RUNTIME.md) — contains AIRuntime service with LoadModel, UnloadModel, Infer, StreamInfer, ListModels, HealthCheck.

---

## orchestrator.proto and agent.proto

Defined in [Phase 5 doc](../phases/05-AGENT-CORE.md) — contains Orchestrator service (goal/task management, agent registration) and Agent service (task execution, health check).

---

## api_gateway.proto

Defined in [Phase 11 doc](../phases/11-API-GATEWAY.md) — contains ApiGateway service (Infer, StreamInfer, GetBudget, GetUsage).

---

## Proto Compilation

### Rust (tonic + prost)
```toml
# agent-core/build.rs
fn main() {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(
            &[
                "proto/common.proto",
                "proto/orchestrator.proto",
                "proto/agent.proto",
                "proto/runtime.proto",
                "proto/tools.proto",
                "proto/memory.proto",
                "proto/api_gateway.proto",
            ],
            &["proto/"],
        )
        .unwrap();
}
```

### Python (grpcio-tools)
See [PYTHON-PACKAGING.md](./PYTHON-PACKAGING.md) for the `generate_proto.sh` script.
