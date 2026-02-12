# Phase 7: Memory System

## Goal
Implement the three-tier memory system (operational → working → long-term) with vector search, context assembly, and decision logging.

## Prerequisites
- Phase 5 complete (orchestrator + agents)
- Read [architecture/MEMORY-SYSTEM.md](../architecture/MEMORY-SYSTEM.md)

---

## Step-by-Step

### Step 7.1: Implement Operational Memory (Rust)

**Claude Code prompt**: "Implement the operational memory module — an in-memory ring buffer with 10K entry capacity, metric tracking, and active task tracking"

```rust
// memory/src/operational.rs
use std::collections::{VecDeque, HashMap};

pub struct OperationalMemory {
    events: VecDeque<Event>,         // Ring buffer, max 10K
    metrics: HashMap<String, f64>,    // Latest value per metric key
    active_tasks: HashMap<String, TaskState>,
    max_entries: usize,
}

impl OperationalMemory {
    pub fn push_event(&mut self, event: Event) {
        if self.events.len() >= self.max_entries {
            let evicted = self.events.pop_front();
            // If critical, flush to working memory
            if evicted.map(|e| e.is_critical()).unwrap_or(false) {
                self.flush_to_working(evicted.unwrap());
            }
        }
        self.events.push_back(event);
    }

    pub fn update_metric(&mut self, key: &str, value: f64) {
        self.metrics.insert(key.to_string(), value);
    }

    pub fn get_recent(&self, count: usize, category: Option<&str>) -> Vec<&Event> {
        self.events.iter().rev()
            .filter(|e| category.map_or(true, |c| e.category == c))
            .take(count)
            .collect()
    }
}
```

### Step 7.2: Implement Working Memory (SQLite)

**Claude Code prompt**: "Implement the working memory module with SQLite — create all tables (goals, tasks, tool_calls, decisions, patterns, agent_states), with async read/write operations"

```rust
// memory/src/working.rs

pub struct WorkingMemory {
    db: rusqlite::Connection,  // /var/lib/aios/memory/working.db
}

impl WorkingMemory {
    pub async fn init(path: &str) -> Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(SCHEMA_SQL)?;  // Create all tables
        Ok(Self { db })
    }

    // Goal operations
    pub async fn store_goal(&self, goal: &Goal) -> Result<()>;
    pub async fn update_goal(&self, id: &str, status: &str) -> Result<()>;
    pub async fn get_active_goals(&self) -> Result<Vec<Goal>>;

    // Task operations
    pub async fn store_task(&self, task: &TaskRecord) -> Result<()>;
    pub async fn get_tasks_for_goal(&self, goal_id: &str) -> Result<Vec<TaskRecord>>;

    // Decision logging
    pub async fn log_decision(&self, decision: &Decision) -> Result<()>;
    pub async fn find_similar_decisions(&self, context: &str) -> Result<Vec<Decision>>;

    // Pattern management
    pub async fn store_pattern(&self, pattern: &Pattern) -> Result<()>;
    pub async fn find_pattern(&self, trigger: &str) -> Result<Option<Pattern>>;
    pub async fn update_pattern_stats(&self, id: &str, success: bool) -> Result<()>;

    // Maintenance
    pub async fn migrate_old_entries(&self, cutoff: DateTime<Utc>) -> Result<u64>;
    pub async fn vacuum(&self) -> Result<()>;
}
```

### Step 7.3: Implement Long-Term Memory (SQLite + ChromaDB)

**Claude Code prompt**: "Implement long-term memory — SQLite for structured archives (procedures, incidents, config history) and ChromaDB for vector search across all historical data"

```python
# memory/python/longterm.py

import chromadb
import aiosqlite

class LongTermMemory:
    def __init__(self, db_path: str, chroma_path: str):
        self.db_path = db_path
        self.chroma = chromadb.PersistentClient(path=chroma_path)

        # Create ChromaDB collections
        self.decisions_collection = self.chroma.get_or_create_collection("decisions")
        self.incidents_collection = self.chroma.get_or_create_collection("incidents")
        self.procedures_collection = self.chroma.get_or_create_collection("procedures")
        self.knowledge_collection = self.chroma.get_or_create_collection("knowledge")

    async def semantic_search(
        self,
        query: str,
        collections: list[str] | None = None,
        n_results: int = 5,
    ) -> list[SearchResult]:
        """Search across all memory using semantic similarity."""
        results = []
        target_collections = collections or ["decisions", "incidents", "procedures"]

        for coll_name in target_collections:
            coll = getattr(self, f"{coll_name}_collection")
            hits = coll.query(query_texts=[query], n_results=n_results)
            for doc, meta, dist in zip(hits["documents"][0], hits["metadatas"][0], hits["distances"][0]):
                results.append(SearchResult(
                    content=doc,
                    metadata=meta,
                    relevance=1.0 - dist,  # Convert distance to similarity
                    collection=coll_name,
                ))

        # Sort by relevance across all collections
        results.sort(key=lambda r: r.relevance, reverse=True)
        return results[:n_results]

    async def store_procedure(self, procedure: Procedure):
        """Store a learned procedure (both structured and vector)."""
        # SQLite: structured data
        async with aiosqlite.connect(self.db_path) as db:
            await db.execute(
                "INSERT INTO procedures (id, name, description, steps, ...) VALUES (?, ?, ?, ?, ...)",
                (procedure.id, procedure.name, procedure.description, json.dumps(procedure.steps))
            )
            await db.commit()

        # ChromaDB: vector embedding for semantic search
        self.procedures_collection.add(
            documents=[f"{procedure.name}: {procedure.description}"],
            metadatas=[{"id": procedure.id, "name": procedure.name}],
            ids=[procedure.id],
        )

    async def store_incident(self, incident: Incident):
        """Store an incident report."""
        # Similar dual storage pattern...
```

### Step 7.4: Implement Context Assembly

**Claude Code prompt**: "Implement the context assembly function that gathers relevant context from all memory tiers for a given task, respecting a token budget"

```python
# memory/python/context.py

async def assemble_context(
    task_description: str,
    max_tokens: int = 4000,
    tiers: list[str] = ["operational", "working", "longterm", "knowledge"],
) -> AssembledContext:
    """Gather relevant context for a task from all memory tiers."""
    chunks = []
    remaining_tokens = max_tokens

    # 1. Operational: current system state (always include, ~500 tokens)
    if "operational" in tiers:
        system_state = await operational_memory.get_system_snapshot()
        chunks.append(ContextChunk(
            source="operational",
            content=format_system_state(system_state),
            relevance=1.0,
        ))
        remaining_tokens -= estimate_tokens(chunks[-1].content)

    # 2. Working: recent relevant tasks and decisions
    if "working" in tiers:
        recent = await working_memory.find_relevant(task_description, limit=5)
        for item in recent:
            chunk = ContextChunk(source="working", content=item.summary, relevance=item.relevance)
            tokens = estimate_tokens(chunk.content)
            if tokens <= remaining_tokens:
                chunks.append(chunk)
                remaining_tokens -= tokens

    # 3. Long-term: historical patterns and similar situations
    if "longterm" in tiers:
        historical = await longterm_memory.semantic_search(task_description, n_results=3)
        for result in historical:
            chunk = ContextChunk(source="longterm", content=result.content, relevance=result.relevance)
            tokens = estimate_tokens(chunk.content)
            if tokens <= remaining_tokens:
                chunks.append(chunk)
                remaining_tokens -= tokens

    # 4. Knowledge base: reference documentation
    if "knowledge" in tiers:
        docs = await knowledge_base.search(task_description, n_results=2)
        for doc in docs:
            chunk = ContextChunk(source="knowledge", content=doc.content, relevance=doc.relevance)
            tokens = estimate_tokens(chunk.content)
            if tokens <= remaining_tokens:
                chunks.append(chunk)
                remaining_tokens -= tokens

    return AssembledContext(
        chunks=chunks,
        total_tokens=max_tokens - remaining_tokens,
    )
```

### Step 7.5: Implement Memory Service (gRPC)

**Claude Code prompt**: "Implement the aios-memory gRPC service that exposes all memory operations to other components"

### Step 7.6: Integrate with Orchestrator

**Claude Code prompt**: "Update the orchestrator to use memory: store goals, tasks, and decisions; assemble context before AI calls; load cached patterns"

### Step 7.7: Data Migration Pipeline

**Claude Code prompt**: "Implement the background data migration: operational → working (on overflow/age), working → long-term (after 30 days), with appropriate summarization"

---

## Deliverables Checklist

- [ ] Operational memory (ring buffer) works with push/query
- [ ] Working memory SQLite schema created and tested
- [ ] Long-term memory SQLite + ChromaDB integrated
- [ ] Semantic search returns relevant results
- [ ] Context assembly respects token budgets
- [ ] Decision logging captures who/what/why
- [ ] Pattern caching stores and retrieves learned patterns
- [ ] Memory service gRPC API accessible by all components
- [ ] Data migration pipeline runs in background
- [ ] Orchestrator uses memory for context before AI calls

---

## Next Phase
→ [Phase 8: Networking](./08-NETWORKING.md)
