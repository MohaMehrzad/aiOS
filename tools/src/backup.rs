//! Backup manager for reversible tool operations

use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;
use uuid::Uuid;

/// Manages pre-execution backups for reversible operations
pub struct BackupManager {
    backup_dir: PathBuf,
    backups: HashMap<String, BackupEntry>,
}

#[allow(dead_code)]
struct BackupEntry {
    execution_id: String,
    tool_name: String,
    backup_path: Option<PathBuf>,
    input_data: Vec<u8>,
    created_at: i64,
}

impl BackupManager {
    pub fn new(backup_dir: &str) -> Self {
        let dir = PathBuf::from(backup_dir);
        let _ = fs::create_dir_all(&dir);
        Self {
            backup_dir: dir,
            backups: HashMap::new(),
        }
    }

    /// Create a backup before a tool execution
    pub fn create_backup(
        &mut self,
        execution_id: &str,
        tool_name: &str,
        input_json: &[u8],
    ) -> String {
        let backup_id = Uuid::new_v4().to_string();

        // For file operations, back up the target file
        let backup_path = if tool_name.starts_with("fs.") {
            self.backup_file_from_input(input_json, &backup_id)
        } else {
            None
        };

        self.backups.insert(
            execution_id.to_string(),
            BackupEntry {
                execution_id: execution_id.to_string(),
                tool_name: tool_name.to_string(),
                backup_path,
                input_data: input_json.to_vec(),
                created_at: chrono::Utc::now().timestamp(),
            },
        );

        info!("Created backup {backup_id} for {tool_name}");
        backup_id
    }

    /// Restore from a backup
    pub async fn rollback(&mut self, execution_id: &str) -> Result<bool> {
        let entry = match self.backups.remove(execution_id) {
            Some(e) => e,
            None => return Ok(false),
        };

        if let Some(backup_path) = &entry.backup_path {
            // Extract target path from input
            if let Ok(input) = serde_json::from_slice::<serde_json::Value>(&entry.input_data) {
                if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                    if backup_path.exists() {
                        fs::copy(backup_path, path)?;
                        fs::remove_file(backup_path)?;
                        info!("Rolled back {} to {}", entry.tool_name, path);
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    /// Back up a file referenced in the tool input
    fn backup_file_from_input(&self, input_json: &[u8], backup_id: &str) -> Option<PathBuf> {
        let input: serde_json::Value = serde_json::from_slice(input_json).ok()?;
        let path = input.get("path")?.as_str()?;

        if Path::new(path).exists() {
            let backup_path = self.backup_dir.join(backup_id);
            if fs::copy(path, &backup_path).is_ok() {
                return Some(backup_path);
            }
        }

        None
    }

    /// Clean old backups
    pub fn cleanup_old(&mut self, max_age_seconds: i64) {
        let now = chrono::Utc::now().timestamp();
        let old_ids: Vec<String> = self
            .backups
            .iter()
            .filter(|(_, e)| now - e.created_at > max_age_seconds)
            .map(|(id, _)| id.clone())
            .collect();

        for id in old_ids {
            if let Some(entry) = self.backups.remove(&id) {
                if let Some(path) = entry.backup_path {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn setup_backup_manager() -> (BackupManager, TempDir) {
        let dir = TempDir::new().unwrap();
        let backup_dir = dir.path().join("backups");
        let bm = BackupManager::new(backup_dir.to_str().unwrap());
        (bm, dir)
    }

    #[test]
    fn test_create_backup_non_fs_tool() {
        let (mut bm, _dir) = setup_backup_manager();
        let backup_id = bm.create_backup("exec-1", "net.ping", b"{}");
        assert!(!backup_id.is_empty());
        assert!(bm.backups.contains_key("exec-1"));
    }

    #[test]
    fn test_create_backup_fs_tool_with_existing_file() {
        let (mut bm, dir) = setup_backup_manager();

        // Create a file to back up
        let target_file = dir.path().join("target.txt");
        {
            let mut f = std::fs::File::create(&target_file).unwrap();
            f.write_all(b"original content").unwrap();
        }

        let input = serde_json::json!({
            "path": target_file.to_str().unwrap()
        });
        let input_bytes = serde_json::to_vec(&input).unwrap();

        let backup_id = bm.create_backup("exec-1", "fs.write", &input_bytes);
        assert!(!backup_id.is_empty());

        let entry = bm.backups.get("exec-1").unwrap();
        assert!(entry.backup_path.is_some());
        assert!(entry.backup_path.as_ref().unwrap().exists());
    }

    #[test]
    fn test_create_backup_fs_tool_no_existing_file() {
        let (mut bm, _dir) = setup_backup_manager();

        let input = serde_json::json!({
            "path": "/nonexistent/file/path.txt"
        });
        let input_bytes = serde_json::to_vec(&input).unwrap();

        let backup_id = bm.create_backup("exec-1", "fs.write", &input_bytes);
        assert!(!backup_id.is_empty());

        let entry = bm.backups.get("exec-1").unwrap();
        assert!(entry.backup_path.is_none());
    }

    #[tokio::test]
    async fn test_rollback_fs_write() {
        let (mut bm, dir) = setup_backup_manager();

        let target_file = dir.path().join("rollback_test.txt");
        {
            let mut f = std::fs::File::create(&target_file).unwrap();
            f.write_all(b"original content").unwrap();
        }

        let input = serde_json::json!({
            "path": target_file.to_str().unwrap()
        });
        let input_bytes = serde_json::to_vec(&input).unwrap();

        bm.create_backup("exec-1", "fs.write", &input_bytes);

        // Simulate modifying the file
        {
            let mut f = std::fs::File::create(&target_file).unwrap();
            f.write_all(b"modified content").unwrap();
        }

        let result = bm.rollback("exec-1").await.unwrap();
        assert!(result);

        let content = std::fs::read_to_string(&target_file).unwrap();
        assert_eq!(content, "original content");
    }

    #[tokio::test]
    async fn test_rollback_nonexistent_execution() {
        let (mut bm, _dir) = setup_backup_manager();
        let result = bm.rollback("nonexistent").await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_rollback_non_fs_tool() {
        let (mut bm, _dir) = setup_backup_manager();
        bm.create_backup("exec-1", "net.ping", b"{}");

        let result = bm.rollback("exec-1").await.unwrap();
        assert!(!result);
    }

    #[test]
    fn test_cleanup_old_entries() {
        let (mut bm, _dir) = setup_backup_manager();

        bm.backups.insert(
            "old-exec".to_string(),
            BackupEntry {
                execution_id: "old-exec".to_string(),
                tool_name: "fs.write".to_string(),
                backup_path: None,
                input_data: vec![],
                created_at: 0, // epoch -- very old
            },
        );
        bm.backups.insert(
            "new-exec".to_string(),
            BackupEntry {
                execution_id: "new-exec".to_string(),
                tool_name: "fs.write".to_string(),
                backup_path: None,
                input_data: vec![],
                created_at: chrono::Utc::now().timestamp(),
            },
        );

        assert_eq!(bm.backups.len(), 2);
        bm.cleanup_old(3600);

        assert_eq!(bm.backups.len(), 1);
        assert!(bm.backups.contains_key("new-exec"));
    }

    #[test]
    fn test_create_multiple_backups() {
        let (mut bm, _dir) = setup_backup_manager();
        for i in 0..5 {
            let exec_id = format!("exec-{i}");
            let backup_id = bm.create_backup(&exec_id, "net.ping", b"{}");
            assert!(!backup_id.is_empty());
        }
        assert_eq!(bm.backups.len(), 5);
    }

    #[test]
    fn test_backup_stores_input_data() {
        let (mut bm, _dir) = setup_backup_manager();
        let input = b"{\"key\":\"value\"}";
        bm.create_backup("exec-1", "net.ping", input);

        let entry = bm.backups.get("exec-1").unwrap();
        assert_eq!(entry.input_data, input.to_vec());
        assert_eq!(entry.tool_name, "net.ping");
        assert_eq!(entry.execution_id, "exec-1");
    }

    #[test]
    fn test_backup_dir_created() {
        let dir = TempDir::new().unwrap();
        let backup_path = dir.path().join("nested").join("backups");
        let _bm = BackupManager::new(backup_path.to_str().unwrap());
        assert!(backup_path.exists());
    }

    #[tokio::test]
    async fn test_rollback_removes_backup_entry() {
        let (mut bm, dir) = setup_backup_manager();

        let target_file = dir.path().join("remove_test.txt");
        {
            let mut f = std::fs::File::create(&target_file).unwrap();
            f.write_all(b"content").unwrap();
        }

        let input = serde_json::json!({"path": target_file.to_str().unwrap()});
        let input_bytes = serde_json::to_vec(&input).unwrap();
        bm.create_backup("exec-1", "fs.write", &input_bytes);

        assert!(bm.backups.contains_key("exec-1"));
        bm.rollback("exec-1").await.unwrap();
        assert!(!bm.backups.contains_key("exec-1"));
    }

    #[test]
    fn test_cleanup_with_no_old_entries() {
        let (mut bm, _dir) = setup_backup_manager();
        bm.backups.insert(
            "recent".to_string(),
            BackupEntry {
                execution_id: "recent".to_string(),
                tool_name: "net.ping".to_string(),
                backup_path: None,
                input_data: vec![],
                created_at: chrono::Utc::now().timestamp(),
            },
        );

        bm.cleanup_old(3600);
        assert_eq!(bm.backups.len(), 1);
    }
}
