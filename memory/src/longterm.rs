//! Long-Term Memory — SQLite + keyword search for persistent data
//!
//! Stores procedures, incidents, config changes.
//! Provides keyword-based search (vector embeddings in future with ChromaDB).

use anyhow::Result;
use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::proto::memory::*;

/// Long-term memory with SQLite storage
pub struct LongTermMemory {
    conn: Mutex<Connection>,
}

impl LongTermMemory {
    pub fn new(db_path: &str) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS procedures (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                steps_json BLOB,
                success_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                avg_duration_ms INTEGER NOT NULL DEFAULT 0,
                tags TEXT,
                created_at INTEGER NOT NULL,
                last_used INTEGER
            );

            CREATE TABLE IF NOT EXISTS incidents (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                symptoms_json BLOB,
                root_cause TEXT,
                resolution TEXT,
                resolved_by TEXT,
                prevention TEXT,
                timestamp INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS config_changes (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                content TEXT NOT NULL,
                changed_by TEXT NOT NULL,
                reason TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_procedures_name ON procedures(name);
            CREATE INDEX IF NOT EXISTS idx_incidents_time ON incidents(timestamp);
            CREATE INDEX IF NOT EXISTS idx_config_path ON config_changes(file_path);",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Keyword-based semantic search across collections
    pub fn semantic_search(
        &self,
        query: &str,
        collections: &[String],
        n_results: i32,
        min_relevance: f64,
    ) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let mut results = Vec::new();
        let limit = if n_results <= 0 { 10 } else { n_results };
        let keywords: Vec<&str> = query.split_whitespace().collect();

        let collections_to_search = if collections.is_empty() {
            vec![
                "procedures".to_string(),
                "incidents".to_string(),
                "config_changes".to_string(),
            ]
        } else {
            collections.to_vec()
        };

        for collection in &collections_to_search {
            match collection.as_str() {
                "procedures" | "decisions" => {
                    let mut stmt = conn.prepare(
                        "SELECT id, name, description FROM procedures ORDER BY last_used DESC LIMIT ?1",
                    )?;
                    let rows = stmt.query_map(params![limit], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?;
                    for row in rows {
                        let (id, name, description) = row?;
                        let content = format!("{name}: {description}");
                        let relevance = keyword_relevance(&keywords, &content);
                        if relevance >= min_relevance {
                            results.push(SearchResult {
                                id,
                                content,
                                metadata_json: vec![],
                                relevance,
                                collection: "procedures".into(),
                            });
                        }
                    }
                }
                "incidents" => {
                    let mut stmt = conn.prepare(
                        "SELECT id, description, root_cause, resolution FROM incidents ORDER BY timestamp DESC LIMIT ?1",
                    )?;
                    let rows = stmt.query_map(params![limit], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                            row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        ))
                    })?;
                    for row in rows {
                        let (id, desc, cause, resolution) = row?;
                        let content = format!("{desc} | Cause: {cause} | Resolution: {resolution}");
                        let relevance = keyword_relevance(&keywords, &content);
                        if relevance >= min_relevance {
                            results.push(SearchResult {
                                id,
                                content,
                                metadata_json: vec![],
                                relevance,
                                collection: "incidents".into(),
                            });
                        }
                    }
                }
                "config_changes" => {
                    let mut stmt = conn.prepare(
                        "SELECT id, file_path, reason FROM config_changes ORDER BY timestamp DESC LIMIT ?1",
                    )?;
                    let rows = stmt.query_map(params![limit], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?;
                    for row in rows {
                        let (id, path, reason) = row?;
                        let content = format!("{path}: {reason}");
                        let relevance = keyword_relevance(&keywords, &content);
                        if relevance >= min_relevance {
                            results.push(SearchResult {
                                id,
                                content,
                                metadata_json: vec![],
                                relevance,
                                collection: "config_changes".into(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // Sort by relevance
        results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);

        Ok(results)
    }

    pub fn store_procedure(&self, procedure: &Procedure) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let tags = procedure.tags.join(",");
        conn.execute(
            "INSERT OR REPLACE INTO procedures (id, name, description, steps_json, success_count, fail_count, avg_duration_ms, tags, created_at, last_used)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                procedure.id,
                procedure.name,
                procedure.description,
                procedure.steps_json,
                procedure.success_count,
                procedure.fail_count,
                procedure.avg_duration_ms,
                tags,
                procedure.created_at,
                procedure.last_used,
            ],
        )?;
        Ok(())
    }

    pub fn store_incident(&self, incident: &Incident) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO incidents (id, description, symptoms_json, root_cause, resolution, resolved_by, prevention, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                incident.id,
                incident.description,
                incident.symptoms_json,
                incident.root_cause,
                incident.resolution,
                incident.resolved_by,
                incident.prevention,
                incident.timestamp,
            ],
        )?;
        Ok(())
    }

    pub fn store_config_change(&self, change: &ConfigChange) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        conn.execute(
            "INSERT INTO config_changes (id, file_path, content, changed_by, reason, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                change.id,
                change.file_path,
                change.content,
                change.changed_by,
                change.reason,
                change.timestamp,
            ],
        )?;
        Ok(())
    }
}

/// Simple keyword-based relevance scoring
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_search_procedure() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        lt.store_procedure(&Procedure {
            id: "proc-1".into(),
            name: "restart_nginx".into(),
            description: "Restart nginx web server when it becomes unresponsive".into(),
            steps_json: b"[]".to_vec(),
            success_count: 5,
            fail_count: 0,
            avg_duration_ms: 2000,
            tags: vec!["nginx".into(), "restart".into()],
            created_at: 1000,
            last_used: 2000,
        })
        .unwrap();

        let results = lt
            .semantic_search("nginx restart", &["procedures".into()], 10, 0.1)
            .unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("nginx"));
    }

    #[test]
    fn test_keyword_relevance() {
        assert_eq!(keyword_relevance(&["hello", "world"], "Hello World"), 1.0);
        assert_eq!(keyword_relevance(&["hello", "world"], "Hello Rust"), 0.5);
        assert_eq!(keyword_relevance(&["hello", "world"], "Rust lang"), 0.0);
    }

    #[test]
    fn test_keyword_relevance_empty_keywords() {
        assert_eq!(keyword_relevance(&[], "Hello World"), 0.5);
    }

    #[test]
    fn test_keyword_relevance_case_insensitive() {
        assert_eq!(keyword_relevance(&["NGINX"], "nginx configuration"), 1.0);
        assert_eq!(keyword_relevance(&["nginx"], "NGINX CONFIG"), 1.0);
    }

    #[test]
    fn test_store_and_search_incident() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        lt.store_incident(&Incident {
            id: "inc-1".into(),
            description: "Nginx service crashed due to memory exhaustion".into(),
            symptoms_json: b"[\"high_memory\", \"oom_kill\"]".to_vec(),
            root_cause: "Memory leak in upstream module".into(),
            resolution: "Restarted nginx and increased memory limit".into(),
            resolved_by: "agent-1".into(),
            prevention: "Add memory monitoring alert".into(),
            timestamp: 1000,
        })
        .unwrap();

        let results = lt
            .semantic_search("nginx memory", &["incidents".into()], 10, 0.1)
            .unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("nginx") || results[0].content.contains("Nginx"));
        assert_eq!(results[0].collection, "incidents");
    }

    #[test]
    fn test_store_and_search_config_change() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        lt.store_config_change(&ConfigChange {
            id: "cfg-1".into(),
            file_path: "/etc/nginx/nginx.conf".into(),
            content: "worker_processes 4;".into(),
            changed_by: "agent-1".into(),
            reason: "Increased worker processes for better throughput".into(),
            timestamp: 1000,
        })
        .unwrap();

        let results = lt
            .semantic_search("nginx config", &["config_changes".into()], 10, 0.1)
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].collection, "config_changes");
    }

    #[test]
    fn test_search_across_collections() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        lt.store_procedure(&Procedure {
            id: "proc-1".into(),
            name: "restart_nginx".into(),
            description: "Restart the nginx web server".into(),
            steps_json: b"[]".to_vec(),
            success_count: 5,
            fail_count: 0,
            avg_duration_ms: 2000,
            tags: vec!["nginx".into()],
            created_at: 1000,
            last_used: 2000,
        })
        .unwrap();

        lt.store_incident(&Incident {
            id: "inc-1".into(),
            description: "Nginx crashed".into(),
            symptoms_json: vec![],
            root_cause: "OOM".into(),
            resolution: "Restart".into(),
            resolved_by: "agent-1".into(),
            prevention: "Monitor".into(),
            timestamp: 1000,
        })
        .unwrap();

        // Search all collections (empty = all)
        let results = lt.semantic_search("nginx", &[], 10, 0.1).unwrap();
        assert!(results.len() >= 2);
    }

    #[test]
    fn test_search_with_no_results() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        let results = lt
            .semantic_search("nonexistent_keyword_xyz", &[], 10, 0.1)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_min_relevance_filtering() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        lt.store_procedure(&Procedure {
            id: "proc-1".into(),
            name: "restart_nginx".into(),
            description: "Restart the nginx web server".into(),
            steps_json: b"[]".to_vec(),
            success_count: 5,
            fail_count: 0,
            avg_duration_ms: 2000,
            tags: vec![],
            created_at: 1000,
            last_used: 2000,
        })
        .unwrap();

        // Query with one matching and one non-matching keyword
        // "nginx" matches but "kubernetes" does not => relevance = 0.5
        let results = lt
            .semantic_search("nginx kubernetes", &["procedures".into()], 10, 0.8)
            .unwrap();
        // Should be filtered out since relevance (0.5) < min_relevance (0.8)
        assert!(results.is_empty());

        let results = lt
            .semantic_search("nginx kubernetes", &["procedures".into()], 10, 0.3)
            .unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_result_limit() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        for i in 0..10 {
            lt.store_procedure(&Procedure {
                id: format!("proc-{i}"),
                name: format!("restart_service_{i}"),
                description: format!("Restart service number {i}"),
                steps_json: b"[]".to_vec(),
                success_count: i,
                fail_count: 0,
                avg_duration_ms: 1000,
                tags: vec!["restart".into()],
                created_at: 1000 + i as i64,
                last_used: 2000 + i as i64,
            })
            .unwrap();
        }

        let results = lt
            .semantic_search("restart service", &["procedures".into()], 3, 0.1)
            .unwrap();
        assert!(results.len() <= 3);
    }

    #[test]
    fn test_search_default_limit() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        // n_results=0 should default to 10
        let results = lt
            .semantic_search("anything", &[], 0, 0.0)
            .unwrap();
        // No data, just verifying it doesn't panic with limit=0
        assert!(results.is_empty());
    }

    #[test]
    fn test_store_procedure_with_tags() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        lt.store_procedure(&Procedure {
            id: "proc-1".into(),
            name: "deploy_app".into(),
            description: "Deploy application to production".into(),
            steps_json: b"[\"build\",\"test\",\"deploy\"]".to_vec(),
            success_count: 10,
            fail_count: 2,
            avg_duration_ms: 60000,
            tags: vec!["deploy".into(), "production".into(), "ci".into()],
            created_at: 1000,
            last_used: 5000,
        })
        .unwrap();

        let results = lt
            .semantic_search("deploy production", &["procedures".into()], 10, 0.1)
            .unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_unknown_collection_ignored() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        // Searching an unknown collection should return no results, not error
        let results = lt
            .semantic_search("anything", &["unknown_collection".into()], 10, 0.0)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_results_sorted_by_relevance() {
        let lt = LongTermMemory::new(":memory:").unwrap();
        lt.store_procedure(&Procedure {
            id: "proc-1".into(),
            name: "nginx_restart".into(),
            description: "Restart nginx web server".into(),
            steps_json: b"[]".to_vec(),
            success_count: 5,
            fail_count: 0,
            avg_duration_ms: 2000,
            tags: vec![],
            created_at: 1000,
            last_used: 2000,
        })
        .unwrap();

        lt.store_procedure(&Procedure {
            id: "proc-2".into(),
            name: "nginx_reload_config".into(),
            description: "Reload nginx configuration after changes to web server config".into(),
            steps_json: b"[]".to_vec(),
            success_count: 3,
            fail_count: 0,
            avg_duration_ms: 500,
            tags: vec![],
            created_at: 1000,
            last_used: 3000,
        })
        .unwrap();

        let results = lt
            .semantic_search("nginx web server", &["procedures".into()], 10, 0.1)
            .unwrap();

        // Results should be sorted by relevance (descending)
        if results.len() >= 2 {
            assert!(results[0].relevance >= results[1].relevance);
        }
    }
}
