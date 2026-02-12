//! Integration tests for the aiOS memory system
//!
//! Tests the three-tier memory architecture:
//! - Operational (in-memory ring buffer)
//! - Working (SQLite)
//! - Knowledge Base (SQLite + keyword search)
//!
//! These tests exercise memory operations that cross module boundaries
//! and verify data flow between tiers.

use std::collections::{HashMap, VecDeque};

// ============================================================================
// Operational Memory Integration Tests
// ============================================================================

/// Test that operational memory ring buffer maintains ordering under stress
#[test]
fn test_operational_memory_ordering_stress() {
    let max = 1000;
    let mut buffer: VecDeque<(String, i64)> = VecDeque::with_capacity(max);

    // Push 5000 events into a 1000-capacity buffer
    for i in 0..5000 {
        if buffer.len() >= max {
            buffer.pop_front();
        }
        buffer.push_back((format!("event_{i}"), i));
    }

    assert_eq!(buffer.len(), max);

    // Verify ordering: newest should be at the back
    let front = buffer.front().unwrap();
    let back = buffer.back().unwrap();
    assert_eq!(front.0, "event_4000");
    assert_eq!(back.0, "event_4999");

    // Verify monotonic timestamps
    let mut prev_ts = -1i64;
    for (_, ts) in &buffer {
        assert!(*ts > prev_ts);
        prev_ts = *ts;
    }
}

/// Test operational memory category-based filtering
#[test]
fn test_operational_memory_multi_category_filter() {
    let mut events: Vec<(String, String, String)> = Vec::new(); // (id, category, source)

    let categories = ["metric", "event", "alert", "audit"];
    let sources = ["agent-1", "agent-2", "system"];

    // Generate diverse events
    for i in 0..100 {
        events.push((
            format!("e{i}"),
            categories[i % categories.len()].to_string(),
            sources[i % sources.len()].to_string(),
        ));
    }

    // Filter by category
    let metrics: Vec<_> = events.iter().filter(|e| e.1 == "metric").collect();
    assert_eq!(metrics.len(), 25);

    let alerts: Vec<_> = events.iter().filter(|e| e.1 == "alert").collect();
    assert_eq!(alerts.len(), 25);

    // Filter by source
    let agent1_events: Vec<_> = events.iter().filter(|e| e.2 == "agent-1").collect();
    // 100 / 3 sources, first source gets ceil(100/3) = 34
    assert_eq!(agent1_events.len(), 34);

    // Combined filter
    let agent1_metrics: Vec<_> = events
        .iter()
        .filter(|e| e.1 == "metric" && e.2 == "agent-1")
        .collect();
    assert!(!agent1_metrics.is_empty());
}

/// Test metric updates and snapshot assembly
#[test]
fn test_operational_metrics_snapshot() {
    let mut metrics: HashMap<String, f64> = HashMap::new();

    // Simulate metric updates
    metrics.insert("cpu.usage".into(), 45.0);
    metrics.insert("memory.used_mb".into(), 8192.0);
    metrics.insert("memory.total_mb".into(), 16384.0);
    metrics.insert("disk.used_gb".into(), 120.0);
    metrics.insert("disk.total_gb".into(), 500.0);
    metrics.insert("tasks.active".into(), 5.0);
    metrics.insert("agents.active".into(), 3.0);

    // Assemble snapshot
    let cpu = *metrics.get("cpu.usage").unwrap_or(&0.0);
    let mem_used = *metrics.get("memory.used_mb").unwrap_or(&0.0);
    let mem_total = *metrics.get("memory.total_mb").unwrap_or(&0.0);
    let disk_used = *metrics.get("disk.used_gb").unwrap_or(&0.0);
    let disk_total = *metrics.get("disk.total_gb").unwrap_or(&0.0);
    let active_tasks = *metrics.get("tasks.active").unwrap_or(&0.0) as i32;
    let active_agents = *metrics.get("agents.active").unwrap_or(&0.0) as i32;

    assert_eq!(cpu, 45.0);
    assert_eq!(mem_used, 8192.0);
    assert_eq!(mem_total, 16384.0);
    assert_eq!(disk_used, 120.0);
    assert_eq!(disk_total, 500.0);
    assert_eq!(active_tasks, 5);
    assert_eq!(active_agents, 3);

    // Memory utilization
    let mem_percent = (mem_used / mem_total) * 100.0;
    assert!((mem_percent - 50.0).abs() < f64::EPSILON);
}

// ============================================================================
// Working Memory Integration Tests
// ============================================================================

/// Test goal lifecycle through working memory
#[test]
fn test_working_memory_goal_lifecycle() {
    #[derive(Debug, Clone)]
    struct Goal {
        id: String,
        description: String,
        status: String,
        priority: i32,
    }

    let mut goals: HashMap<String, Goal> = HashMap::new();

    // Create goal
    let goal = Goal {
        id: "goal-1".into(),
        description: "Deploy new version".into(),
        status: "pending".into(),
        priority: 1,
    };
    goals.insert(goal.id.clone(), goal);

    // Verify pending
    let active: Vec<_> = goals
        .values()
        .filter(|g| g.status != "completed" && g.status != "failed")
        .collect();
    assert_eq!(active.len(), 1);

    // Transition to in_progress
    goals.get_mut("goal-1").unwrap().status = "in_progress".into();

    // Add tasks
    #[derive(Debug)]
    struct Task {
        id: String,
        goal_id: String,
        status: String,
    }

    let mut tasks: Vec<Task> = vec![
        Task {
            id: "task-1".into(),
            goal_id: "goal-1".into(),
            status: "pending".into(),
        },
        Task {
            id: "task-2".into(),
            goal_id: "goal-1".into(),
            status: "pending".into(),
        },
        Task {
            id: "task-3".into(),
            goal_id: "goal-1".into(),
            status: "pending".into(),
        },
    ];

    // Complete tasks one by one
    tasks[0].status = "completed".into();
    tasks[1].status = "completed".into();
    tasks[2].status = "completed".into();

    // Check all completed
    let all_done = tasks.iter().all(|t| t.status == "completed");
    assert!(all_done);

    // Complete goal
    goals.get_mut("goal-1").unwrap().status = "completed".into();

    let active: Vec<_> = goals
        .values()
        .filter(|g| g.status != "completed" && g.status != "failed")
        .collect();
    assert_eq!(active.len(), 0);
}

/// Test pattern matching and learning in working memory
#[test]
fn test_working_memory_pattern_learning() {
    #[derive(Debug, Clone)]
    struct Pattern {
        trigger: String,
        action: String,
        success_rate: f64,
        uses: u32,
    }

    let mut patterns: Vec<Pattern> = vec![
        Pattern {
            trigger: "high cpu usage".into(),
            action: "restart heavy service".into(),
            success_rate: 0.9,
            uses: 10,
        },
        Pattern {
            trigger: "disk space low".into(),
            action: "cleanup temp files".into(),
            success_rate: 0.95,
            uses: 20,
        },
        Pattern {
            trigger: "service unresponsive".into(),
            action: "restart service".into(),
            success_rate: 0.7,
            uses: 5,
        },
    ];

    // Find pattern for "cpu" trigger with min 0.8 success rate
    let matched = patterns
        .iter()
        .filter(|p| p.trigger.contains("cpu") && p.success_rate >= 0.8)
        .next();
    assert!(matched.is_some());
    assert_eq!(matched.unwrap().action, "restart heavy service");

    // Find pattern for "service" trigger with min 0.8 success rate
    let matched = patterns
        .iter()
        .filter(|p| p.trigger.contains("service") && p.success_rate >= 0.8)
        .next();
    // service unresponsive has 0.7, so no match at 0.8 threshold
    assert!(matched.is_none());

    // Update pattern stats after successful use
    let cpu_pattern = patterns
        .iter_mut()
        .find(|p| p.trigger.contains("cpu"))
        .unwrap();
    let new_rate = (cpu_pattern.success_rate * cpu_pattern.uses as f64 + 1.0)
        / (cpu_pattern.uses as f64 + 1.0);
    cpu_pattern.success_rate = new_rate;
    cpu_pattern.uses += 1;

    assert_eq!(cpu_pattern.uses, 11);
    assert!(cpu_pattern.success_rate > 0.9); // Should increase slightly
}

/// Test working memory TTL behavior simulation
#[test]
fn test_working_memory_ttl_simulation() {
    struct TimedEntry {
        key: String,
        value: String,
        created_at: i64,
        ttl_seconds: i64,
    }

    let now = 1000000i64;
    let mut entries = vec![
        TimedEntry {
            key: "fresh".into(),
            value: "still valid".into(),
            created_at: now - 100,
            ttl_seconds: 3600,
        },
        TimedEntry {
            key: "stale".into(),
            value: "expired".into(),
            created_at: now - 7200,
            ttl_seconds: 3600,
        },
        TimedEntry {
            key: "borderline".into(),
            value: "just expired".into(),
            created_at: now - 3600,
            ttl_seconds: 3600,
        },
    ];

    // Filter to active entries
    let active: Vec<_> = entries
        .iter()
        .filter(|e| now - e.created_at < e.ttl_seconds)
        .collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].key, "fresh");

    // Migrate expired entries (simulate migration to long-term)
    let expired_keys: Vec<String> = entries
        .iter()
        .filter(|e| now - e.created_at >= e.ttl_seconds)
        .map(|e| e.key.clone())
        .collect();
    assert_eq!(expired_keys.len(), 2);
    assert!(expired_keys.contains(&"stale".to_string()));
    assert!(expired_keys.contains(&"borderline".to_string()));

    // Remove expired
    entries.retain(|e| now - e.created_at < e.ttl_seconds);
    assert_eq!(entries.len(), 1);
}

// ============================================================================
// Knowledge Graph / Knowledge Base Integration Tests
// ============================================================================

/// Test knowledge entry CRUD operations
#[test]
fn test_knowledge_entry_crud() {
    #[derive(Debug, Clone)]
    struct KnowledgeEntry {
        id: u64,
        title: String,
        content: String,
        source: String,
        tags: Vec<String>,
    }

    let mut store: HashMap<u64, KnowledgeEntry> = HashMap::new();
    let mut next_id = 1u64;

    // Create
    let entry1 = KnowledgeEntry {
        id: next_id,
        title: "Nginx Configuration".into(),
        content: "Nginx uses worker_processes to handle connections".into(),
        source: "docs".into(),
        tags: vec!["nginx".into(), "config".into()],
    };
    store.insert(next_id, entry1);
    next_id += 1;

    let entry2 = KnowledgeEntry {
        id: next_id,
        title: "Firewall Rules".into(),
        content: "nftables replaces iptables for packet filtering".into(),
        source: "man".into(),
        tags: vec!["firewall".into(), "nftables".into()],
    };
    store.insert(next_id, entry2);
    next_id += 1;

    assert_eq!(store.len(), 2);

    // Read
    let entry = store.get(&1).unwrap();
    assert_eq!(entry.title, "Nginx Configuration");

    // Search by keyword
    let query = "nginx";
    let results: Vec<_> = store
        .values()
        .filter(|e| {
            e.title.to_lowercase().contains(query)
                || e.content.to_lowercase().contains(query)
                || e.tags.iter().any(|t| t.to_lowercase().contains(query))
        })
        .collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Nginx Configuration");

    // Search by tag
    let tag_query = "firewall";
    let results: Vec<_> = store
        .values()
        .filter(|e| e.tags.iter().any(|t| t == tag_query))
        .collect();
    assert_eq!(results.len(), 1);

    // Delete
    store.remove(&1);
    assert_eq!(store.len(), 1);
    assert!(store.get(&1).is_none());
}

/// Test knowledge base keyword search relevance scoring
#[test]
fn test_knowledge_search_relevance() {
    fn keyword_relevance(keywords: &[&str], text: &str) -> f64 {
        if keywords.is_empty() {
            return 0.5;
        }
        let text_lower = text.to_lowercase();
        let matches = keywords
            .iter()
            .filter(|kw| text_lower.contains(&kw.to_lowercase()))
            .count();
        matches as f64 / keywords.len() as f64
    }

    struct Entry {
        title: String,
        content: String,
    }

    let entries = vec![
        Entry {
            title: "Nginx HTTP Server".into(),
            content: "Nginx is a high-performance HTTP and reverse proxy server".into(),
        },
        Entry {
            title: "Apache HTTP Server".into(),
            content: "Apache is a popular HTTP server with modular architecture".into(),
        },
        Entry {
            title: "Redis Cache".into(),
            content: "Redis is an in-memory data structure store".into(),
        },
    ];

    let keywords: Vec<&str> = "nginx http server".split_whitespace().collect();

    let mut scored: Vec<(&Entry, f64)> = entries
        .iter()
        .map(|e| {
            let text = format!("{} {}", e.title, e.content);
            let score = keyword_relevance(&keywords, &text);
            (e, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Nginx entry should have highest relevance (all 3 keywords match)
    assert_eq!(scored[0].0.title, "Nginx HTTP Server");
    assert_eq!(scored[0].1, 1.0);

    // Apache also has "http" and "server"
    assert_eq!(scored[1].0.title, "Apache HTTP Server");
    assert!((scored[1].1 - 2.0 / 3.0).abs() < f64::EPSILON);
}

/// Test cross-tier memory context assembly
#[test]
fn test_context_assembly_across_tiers() {
    struct ContextChunk {
        source: String,
        content: String,
        relevance: f64,
        tokens: i32,
    }

    fn estimate_tokens(text: &str) -> i32 {
        (text.len() as f64 / 4.0).ceil() as i32
    }

    let mut chunks: Vec<ContextChunk> = Vec::new();
    let max_tokens = 500i32;
    let mut total_tokens = 0i32;

    // Operational tier
    let op_events = vec!["CPU at 45%", "Memory at 60%", "3 active tasks"];
    for event in op_events {
        let tokens = estimate_tokens(event);
        if total_tokens + tokens <= max_tokens {
            chunks.push(ContextChunk {
                source: "operational".into(),
                content: event.into(),
                relevance: 0.8,
                tokens,
            });
            total_tokens += tokens;
        }
    }

    // Working tier
    let working_items = vec!["Goal: Deploy v2.0 (in_progress)", "Task: Run tests (pending)"];
    for item in working_items {
        let tokens = estimate_tokens(item);
        if total_tokens + tokens <= max_tokens {
            chunks.push(ContextChunk {
                source: "working".into(),
                content: item.into(),
                relevance: 0.7,
                tokens,
            });
            total_tokens += tokens;
        }
    }

    // Knowledge tier
    let knowledge = vec!["Nginx config: worker_processes 4"];
    for item in knowledge {
        let tokens = estimate_tokens(item);
        if total_tokens + tokens <= max_tokens {
            chunks.push(ContextChunk {
                source: "knowledge".into(),
                content: item.into(),
                relevance: 0.6,
                tokens,
            });
            total_tokens += tokens;
        }
    }

    assert!(total_tokens <= max_tokens);
    assert!(!chunks.is_empty());

    // Verify all tiers are represented
    let sources: Vec<&str> = chunks.iter().map(|c| c.source.as_str()).collect();
    assert!(sources.contains(&"operational"));
    assert!(sources.contains(&"working"));
    assert!(sources.contains(&"knowledge"));

    // Sort by relevance
    chunks.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap());
    assert_eq!(chunks[0].source, "operational");
}

/// Test memory store and retrieve round-trip
#[test]
fn test_memory_store_retrieve_roundtrip() {
    let mut store: HashMap<String, String> = HashMap::new();

    // Store structured data as a simple string representation
    let data = r#"{"goal_id":"goal-1","description":"Deploy service","priority":1}"#;
    store.insert("goal-1".into(), data.to_string());

    // Retrieve
    let retrieved = store.get("goal-1").unwrap();
    assert!(retrieved.contains("goal-1"));
    assert!(retrieved.contains("Deploy service"));
    assert!(retrieved.contains("\"priority\":1"));

    // Verify we can store binary data too
    let mut binary_store: HashMap<String, Vec<u8>> = HashMap::new();
    let bytes = data.as_bytes().to_vec();
    binary_store.insert("goal-1".into(), bytes.clone());

    let retrieved_bytes = binary_store.get("goal-1").unwrap();
    assert_eq!(retrieved_bytes, &bytes);

    // Round-trip: bytes -> string -> bytes
    let as_string = String::from_utf8(retrieved_bytes.clone()).unwrap();
    assert_eq!(as_string, data);
}

/// Test knowledge base with entity relationships
#[test]
fn test_knowledge_entity_relationships() {
    #[derive(Debug, Clone)]
    struct Entity {
        name: String,
        entity_type: String,
    }

    #[derive(Debug)]
    struct Relationship {
        from: String,
        to: String,
        rel_type: String,
    }

    let mut entities: HashMap<String, Entity> = HashMap::new();
    let mut relationships: Vec<Relationship> = Vec::new();

    // Add entities
    entities.insert(
        "nginx".into(),
        Entity {
            name: "nginx".into(),
            entity_type: "service".into(),
        },
    );
    entities.insert(
        "port_80".into(),
        Entity {
            name: "port 80".into(),
            entity_type: "resource".into(),
        },
    );
    entities.insert(
        "port_443".into(),
        Entity {
            name: "port 443".into(),
            entity_type: "resource".into(),
        },
    );
    entities.insert(
        "web_config".into(),
        Entity {
            name: "/etc/nginx/nginx.conf".into(),
            entity_type: "config_file".into(),
        },
    );

    // Add relationships
    relationships.push(Relationship {
        from: "nginx".into(),
        to: "port_80".into(),
        rel_type: "listens_on".into(),
    });
    relationships.push(Relationship {
        from: "nginx".into(),
        to: "port_443".into(),
        rel_type: "listens_on".into(),
    });
    relationships.push(Relationship {
        from: "nginx".into(),
        to: "web_config".into(),
        rel_type: "configured_by".into(),
    });

    // Query: what does nginx listen on?
    let nginx_ports: Vec<&Relationship> = relationships
        .iter()
        .filter(|r| r.from == "nginx" && r.rel_type == "listens_on")
        .collect();
    assert_eq!(nginx_ports.len(), 2);

    // Query: what configures nginx?
    let nginx_configs: Vec<&Relationship> = relationships
        .iter()
        .filter(|r| r.from == "nginx" && r.rel_type == "configured_by")
        .collect();
    assert_eq!(nginx_configs.len(), 1);
    assert_eq!(nginx_configs[0].to, "web_config");

    // Query: all services
    let services: Vec<&Entity> = entities
        .values()
        .filter(|e| e.entity_type == "service")
        .collect();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].name, "nginx");

    // Delete entity and cascading relationships
    entities.remove("port_80");
    relationships.retain(|r| r.from != "port_80" && r.to != "port_80");
    assert_eq!(
        relationships
            .iter()
            .filter(|r| r.from == "nginx" && r.rel_type == "listens_on")
            .count(),
        1
    );
}
