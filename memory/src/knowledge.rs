//! Knowledge Base — stores learned facts, documentation, procedures
//!
//! Keyword-based search for now, will add vector embeddings via ChromaDB later.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::proto::memory::*;

/// In-process knowledge base with SQLite storage
pub struct KnowledgeBase {
    conn: Mutex<Connection>,
}

impl KnowledgeBase {
    pub fn new() -> Result<Self> {
        let conn = Connection::open_in_memory()?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS knowledge (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                tags TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_knowledge_title ON knowledge(title);
            CREATE INDEX IF NOT EXISTS idx_knowledge_source ON knowledge(source);",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Add a knowledge entry
    pub fn add_entry(&mut self, entry: &KnowledgeEntry) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let tags = entry.tags.join(",");
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO knowledge (title, content, source, tags, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entry.title, entry.content, entry.source, tags, now],
        )?;

        Ok(())
    }

    /// Search knowledge base by keywords
    pub fn search(&self, query: &str, n_results: i32) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let limit = if n_results <= 0 { 10 } else { n_results };
        let keywords: Vec<&str> = query.split_whitespace().collect();

        let mut stmt = conn.prepare(
            "SELECT rowid, title, content, source, tags FROM knowledge ORDER BY created_at DESC LIMIT ?1",
        )?;

        let mut results: Vec<SearchResult> = Vec::new();
        let rows = stmt.query_map(params![limit * 3], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            ))
        })?;

        for row in rows {
            let (id, title, content, source, tags) = row?;
            let full_text = format!("{title} {content} {tags}");
            let relevance = keyword_relevance(&keywords, &full_text);

            if relevance > 0.0 {
                results.push(SearchResult {
                    id: id.to_string(),
                    content: format!("[{source}] {title}: {content}"),
                    metadata_json: serde_json::to_vec(&serde_json::json!({
                        "source": source,
                        "tags": tags,
                    }))
                    .unwrap_or_default(),
                    relevance,
                    collection: "knowledge".into(),
                });
            }
        }

        results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);

        Ok(results)
    }
}

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
    fn test_add_and_search() {
        let mut kb = KnowledgeBase::new().unwrap();
        kb.add_entry(&KnowledgeEntry {
            title: "Nginx Configuration".into(),
            content: "Nginx serves HTTP traffic on port 80 and HTTPS on 443".into(),
            source: "man page".into(),
            tags: vec!["nginx".into(), "http".into()],
        })
        .unwrap();

        kb.add_entry(&KnowledgeEntry {
            title: "Firewall Rules".into(),
            content: "nftables manages firewall rules for packet filtering".into(),
            source: "docs".into(),
            tags: vec!["firewall".into(), "nftables".into()],
        })
        .unwrap();

        let results = kb.search("nginx http", 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("Nginx"));

        let results = kb.search("firewall", 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("nftables"));
    }

    #[test]
    fn test_search_no_results() {
        let kb = KnowledgeBase::new().unwrap();
        let results = kb.search("nonexistent_xyz", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_empty_database() {
        let kb = KnowledgeBase::new().unwrap();
        let results = kb.search("anything", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_default_limit() {
        let kb = KnowledgeBase::new().unwrap();
        // n_results=0 should default to 10
        let results = kb.search("anything", 0).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_result_limit() {
        let mut kb = KnowledgeBase::new().unwrap();
        for i in 0..10 {
            kb.add_entry(&KnowledgeEntry {
                title: format!("Topic {i}"),
                content: format!("This is about topic {i} with keyword searchable"),
                source: "docs".into(),
                tags: vec!["searchable".into()],
            })
            .unwrap();
        }

        let results = kb.search("searchable topic", 3).unwrap();
        assert!(results.len() <= 3);
    }

    #[test]
    fn test_search_by_tags() {
        let mut kb = KnowledgeBase::new().unwrap();
        kb.add_entry(&KnowledgeEntry {
            title: "Kubernetes".into(),
            content: "Container orchestration platform".into(),
            source: "docs".into(),
            tags: vec!["k8s".into(), "container".into()],
        })
        .unwrap();

        // Search by tag content
        let results = kb.search("k8s", 10).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_results_sorted_by_relevance() {
        let mut kb = KnowledgeBase::new().unwrap();
        kb.add_entry(&KnowledgeEntry {
            title: "Nginx HTTP Server".into(),
            content: "Nginx serves HTTP traffic and handles reverse proxy".into(),
            source: "docs".into(),
            tags: vec!["nginx".into(), "http".into()],
        })
        .unwrap();

        kb.add_entry(&KnowledgeEntry {
            title: "Docker".into(),
            content: "Docker is a containerization platform for nginx and other services".into(),
            source: "docs".into(),
            tags: vec!["docker".into()],
        })
        .unwrap();

        let results = kb.search("nginx http", 10).unwrap();
        // Results should be sorted by relevance descending
        if results.len() >= 2 {
            assert!(results[0].relevance >= results[1].relevance);
        }
    }

    #[test]
    fn test_search_result_metadata() {
        let mut kb = KnowledgeBase::new().unwrap();
        kb.add_entry(&KnowledgeEntry {
            title: "Test Entry".into(),
            content: "Some content for testing".into(),
            source: "manual".into(),
            tags: vec!["test".into()],
        })
        .unwrap();

        let results = kb.search("test", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].collection, "knowledge");
        assert!(results[0].content.contains("manual")); // Source embedded in content
        assert!(results[0].content.contains("Test Entry"));
        assert!(!results[0].metadata_json.is_empty());
    }

    #[test]
    fn test_keyword_relevance_empty() {
        assert_eq!(keyword_relevance(&[], "anything"), 0.5);
    }

    #[test]
    fn test_keyword_relevance_partial_match() {
        // 1 out of 3 keywords match
        let score = keyword_relevance(&["nginx", "redis", "postgres"], "nginx configuration");
        assert!((score - 1.0 / 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_keyword_relevance_no_match() {
        let score = keyword_relevance(&["foo", "bar"], "completely unrelated text");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_add_multiple_entries() {
        let mut kb = KnowledgeBase::new().unwrap();
        for i in 0..20 {
            kb.add_entry(&KnowledgeEntry {
                title: format!("Entry {i}"),
                content: format!("Content for entry number {i}"),
                source: "batch".into(),
                tags: vec![],
            })
            .unwrap();
        }

        let results = kb.search("entry content", 100).unwrap();
        assert_eq!(results.len(), 20);
    }

    #[test]
    fn test_search_source_filtering_in_content() {
        let mut kb = KnowledgeBase::new().unwrap();
        kb.add_entry(&KnowledgeEntry {
            title: "API Docs".into(),
            content: "REST API documentation for the service".into(),
            source: "swagger".into(),
            tags: vec!["api".into()],
        })
        .unwrap();

        let results = kb.search("API", 10).unwrap();
        assert!(!results.is_empty());
        // Content format is "[source] title: content"
        assert!(results[0].content.contains("[swagger]"));
    }
}
