# Memory System Architecture

## Overview

The Memory System gives aiOS perfect recall. Every decision, action, and outcome is stored and retrievable. This is what makes the AI get smarter over time — it learns from its own history.

---

## Memory Tiers

```
┌─────────────────────────────────────────────────────────────┐
│                    MEMORY HIERARCHY                          │
│                                                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  OPERATIONAL MEMORY          (In-Memory / Ring Buffer) │  │
│  │  TTL: 1 hour | Size: 10K entries | Latency: <1ms      │  │
│  │  What: Last hour of events, active tasks, live metrics │  │
│  └───────────────────────────────────────────────────────┘  │
│                           │ Overflow                         │
│  ┌───────────────────────▼───────────────────────────────┐  │
│  │  WORKING MEMORY              (SQLite)                  │  │
│  │  TTL: 30 days | Size: ~1GB | Latency: <5ms            │  │
│  │  What: Current goals, task history, agent state,       │  │
│  │        recent decisions, cached tool results            │  │
│  └───────────────────────────────────────────────────────┘  │
│                           │ Aging                            │
│  ┌───────────────────────▼───────────────────────────────┐  │
│  │  LONG-TERM MEMORY            (SQLite + ChromaDB)       │  │
│  │  TTL: Forever | Size: ~50GB | Latency: <50ms           │  │
│  │  What: All historical data, learned patterns,          │  │
│  │        semantic search, knowledge base                  │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  KNOWLEDGE BASE              (Embedded Docs + Vectors)  │  │
│  │  What: System docs, man pages, package docs,           │  │
│  │        learned procedures, best practices               │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## Operational Memory (Hot)

**Implementation**: In-memory ring buffer (Rust `VecDeque`)

**Purpose**: Real-time system awareness. What's happening RIGHT NOW.

### Contents
- Active process list and resource usage
- Current network connections
- Live metric values (CPU, RAM, disk, GPU)
- Pending and in-progress tasks
- Recent tool call results (last 100)
- Active agent states

### Schema
```rust
struct OperationalEntry {
    id: u64,
    timestamp: Instant,
    category: Category,  // metric, event, task, tool_result
    data: Value,         // serde_json::Value
}

struct OperationalMemory {
    entries: VecDeque<OperationalEntry>,  // Ring buffer, max 10K
    metrics: HashMap<String, MetricValue>, // Latest value per metric
    active_tasks: HashMap<TaskId, TaskState>,
}
```

### Eviction
- FIFO — oldest entries drop off when buffer is full
- Critical entries (errors, security events) are immediately flushed to working memory

---

## Working Memory (Warm)

**Implementation**: SQLite database at `/var/lib/aios/memory/working.db`

**Purpose**: Current operational context. What's the AI working on and what happened recently.

### Tables

```sql
-- Current and recent goals
CREATE TABLE goals (
    id TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    status TEXT NOT NULL,  -- pending, active, completed, failed
    priority INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL,
    completed_at TIMESTAMP,
    result TEXT,
    metadata JSON
);

-- Task execution history
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    goal_id TEXT REFERENCES goals(id),
    description TEXT NOT NULL,
    agent TEXT NOT NULL,
    status TEXT NOT NULL,
    input JSON,
    output JSON,
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    duration_ms INTEGER,
    error TEXT
);

-- Tool call log
CREATE TABLE tool_calls (
    id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(id),
    tool_name TEXT NOT NULL,
    agent TEXT NOT NULL,
    input JSON NOT NULL,
    output JSON,
    success BOOLEAN,
    duration_ms INTEGER,
    reason TEXT,
    timestamp TIMESTAMP NOT NULL
);

-- Agent state snapshots
CREATE TABLE agent_states (
    agent_name TEXT NOT NULL,
    state JSON NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    PRIMARY KEY (agent_name)
);

-- Decision log (WHY was each decision made)
CREATE TABLE decisions (
    id TEXT PRIMARY KEY,
    context TEXT NOT NULL,        -- What situation triggered the decision
    options JSON,                 -- What options were considered
    chosen TEXT NOT NULL,         -- What was decided
    reasoning TEXT NOT NULL,      -- WHY this was chosen
    intelligence_level TEXT,      -- reactive/operational/tactical/strategic
    model_used TEXT,              -- Which model made the decision
    outcome TEXT,                 -- Did it work? (updated later)
    timestamp TIMESTAMP NOT NULL
);

-- Cached patterns (learned shortcuts)
CREATE TABLE patterns (
    id TEXT PRIMARY KEY,
    trigger TEXT NOT NULL,        -- What situation triggers this pattern
    action TEXT NOT NULL,         -- What to do
    success_rate REAL DEFAULT 1.0,
    uses INTEGER DEFAULT 0,
    last_used TIMESTAMP,
    created_from TEXT             -- Which goal/task first created this pattern
);
```

### Maintenance
- Entries older than 30 days are migrated to long-term memory
- High-value entries (successful patterns, important decisions) are always kept
- Vacuum runs weekly to reclaim space

---

## Long-Term Memory (Cold)

**Implementation**: SQLite (structured data) + ChromaDB (vector embeddings)

**Purpose**: Everything that ever happened. Semantic search across all history.

### SQLite (`/var/lib/aios/memory/longterm.db`)
Archived versions of all working memory tables, plus:

```sql
-- Learned procedures (multi-step solutions that worked)
CREATE TABLE procedures (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    steps JSON NOT NULL,          -- Ordered list of tool calls
    success_count INTEGER DEFAULT 0,
    fail_count INTEGER DEFAULT 0,
    avg_duration_ms INTEGER,
    tags TEXT[],                   -- For categorization
    created_at TIMESTAMP,
    last_used TIMESTAMP
);

-- System configuration history
CREATE TABLE config_history (
    id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    content TEXT NOT NULL,
    changed_by TEXT,              -- Which agent/goal
    reason TEXT,
    timestamp TIMESTAMP NOT NULL
);

-- Incident reports (things that went wrong and how they were fixed)
CREATE TABLE incidents (
    id TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    symptoms JSON,
    root_cause TEXT,
    resolution TEXT,
    resolved_by TEXT,
    prevention TEXT,              -- What to do to prevent recurrence
    timestamp TIMESTAMP NOT NULL
);
```

### ChromaDB (`/var/lib/aios/vectors/`)
Vector database for semantic search across all memory.

**What gets embedded:**
- Goal descriptions
- Task descriptions and results
- Decision reasoning
- Incident reports
- Procedure descriptions
- Tool call reasons

**Use case**: Agent faces a new problem → searches long-term memory → finds similar past situations and what worked.

```python
# Example: Finding relevant past experience
results = await memory.semantic_search(
    query="nginx returning 502 bad gateway after restart",
    collections=["incidents", "decisions", "procedures"],
    n_results=5
)
# Returns similar incidents from the past with their resolutions
```

---

## Knowledge Base

**Implementation**: Pre-embedded documentation + runtime additions

**Purpose**: Reference material the AI can search. Not its own experience — external knowledge.

### Pre-loaded Content
- Linux man pages (core commands)
- Package documentation for installed software
- Network protocol references
- Security best practices (OWASP, CIS benchmarks)
- aiOS internal documentation

### Runtime Additions
- Documentation fetched when new packages are installed
- Solutions found via API calls that prove useful
- User-provided documentation and runbooks

### Access Pattern
```python
# Agent needs to configure nginx
docs = await knowledge_base.search(
    query="nginx reverse proxy configuration upstream",
    n_results=3
)
# Returns relevant nginx documentation chunks
```

---

## Memory Service API

The Memory System runs as a gRPC service (`aios-memory`).

```protobuf
service MemoryService {
    // Operational memory
    rpc PushEvent(Event) returns (Empty);
    rpc GetRecentEvents(GetRecentRequest) returns (EventList);
    rpc GetMetric(MetricRequest) returns (MetricValue);

    // Working memory
    rpc StoreGoal(Goal) returns (Empty);
    rpc StoreTask(Task) returns (Empty);
    rpc StoreToolCall(ToolCallRecord) returns (Empty);
    rpc StoreDecision(Decision) returns (Empty);
    rpc QueryWorking(QueryRequest) returns (QueryResponse);

    // Long-term memory
    rpc SemanticSearch(SearchRequest) returns (SearchResponse);
    rpc StoreProcedure(Procedure) returns (Empty);
    rpc StoreIncident(Incident) returns (Empty);
    rpc QueryLongTerm(QueryRequest) returns (QueryResponse);

    // Knowledge base
    rpc SearchKnowledge(SearchRequest) returns (SearchResponse);
    rpc AddKnowledge(KnowledgeEntry) returns (Empty);

    // Context assembly (get relevant context for a task)
    rpc AssembleContext(ContextRequest) returns (ContextResponse);
}

message ContextRequest {
    string task_description = 1;
    int32 max_tokens = 2;
    repeated string memory_tiers = 3;  // which tiers to search
}

message ContextResponse {
    repeated ContextChunk chunks = 1;
    int32 total_tokens = 2;
}
```

---

## Context Assembly

Before an agent or the orchestrator calls an AI model, the Memory System assembles relevant context:

```
1. Parse the current task/question
2. Search operational memory for real-time relevant data
3. Search working memory for recent relevant tasks/decisions
4. Search long-term memory for historical patterns
5. Search knowledge base for reference material
6. Rank by relevance, deduplicate
7. Trim to token budget
8. Return as structured context
```

This ensures every AI call has maximum useful context without exceeding token limits.

---

## Data Lifecycle

```
Event occurs
    │
    ▼
Operational Memory (in-memory, 1hr)
    │ after 1hr or buffer full
    ▼
Working Memory (SQLite, 30 days)
    │ after 30 days
    ▼
Long-Term Memory (SQLite + ChromaDB, forever)
    │ optional
    ▼
Compressed Archive (yearly, for storage efficiency)
```

### Retention Policies
| Data Type | Working Memory | Long-Term | Notes |
|---|---|---|---|
| Metrics | 7 days | 1 year (aggregated) | Raw → hourly averages |
| Tool calls | 30 days | Forever | Full audit trail |
| Decisions | 30 days | Forever | Learning data |
| Goals/Tasks | 90 days | Forever | Compressed |
| Incidents | Forever | Forever | Always kept |
| Patterns | Forever | Forever | Active patterns never age out |
