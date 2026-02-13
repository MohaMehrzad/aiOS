//! sec.file_integrity — SHA256 checksums of critical files vs baseline DB

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct IntegrityInput {
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_paths")]
    paths: Vec<String>,
}

fn default_mode() -> String {
    "check".into()
}

fn default_paths() -> Vec<String> {
    vec![
        "/etc/aios".into(),
        "/etc/passwd".into(),
        "/etc/shadow".into(),
        "/etc/ssh/sshd_config".into(),
    ]
}

#[derive(Serialize)]
struct IntegrityOutput {
    mode: String,
    checked: usize,
    modified: Vec<IntegrityChange>,
    new_files: Vec<String>,
    missing_files: Vec<String>,
}

#[derive(Serialize)]
struct IntegrityChange {
    path: String,
    expected: String,
    actual: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: IntegrityInput =
        serde_json::from_slice(input).context("Invalid sec.file_integrity input")?;

    let db_path = "/var/lib/aios/data/file_integrity.db";
    std::fs::create_dir_all("/var/lib/aios/data").ok();
    let conn =
        rusqlite::Connection::open(db_path).context("Failed to open integrity database")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS file_checksums (
            path TEXT PRIMARY KEY,
            sha256 TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .context("Failed to create checksums table")?;

    match req.mode.as_str() {
        "baseline" => create_baseline(&conn, &req.paths),
        "check" | _ => check_integrity(&conn, &req.paths),
    }
}

fn hash_file(path: &str) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Some(format!("{:x}", hasher.finalize()))
}

fn create_baseline(conn: &rusqlite::Connection, paths: &[String]) -> Result<Vec<u8>> {
    let mut checked = 0;
    let now = chrono::Utc::now().to_rfc3339();

    for path in paths {
        let entries = collect_files(path);
        for file_path in entries {
            if let Some(hash) = hash_file(&file_path) {
                conn.execute(
                    "INSERT OR REPLACE INTO file_checksums (path, sha256, updated_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params![file_path, hash, now],
                )?;
                checked += 1;
            }
        }
    }

    let output = IntegrityOutput {
        mode: "baseline".into(),
        checked,
        modified: vec![],
        new_files: vec![],
        missing_files: vec![],
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}

fn check_integrity(conn: &rusqlite::Connection, paths: &[String]) -> Result<Vec<u8>> {
    let mut checked = 0;
    let mut modified = Vec::new();
    let mut new_files = Vec::new();
    let mut missing_files = Vec::new();

    for path in paths {
        let entries = collect_files(path);
        for file_path in entries {
            checked += 1;
            let current_hash = hash_file(&file_path);

            let stored: Option<String> = conn
                .query_row(
                    "SELECT sha256 FROM file_checksums WHERE path = ?1",
                    [&file_path],
                    |row| row.get(0),
                )
                .ok();

            match (stored, current_hash) {
                (Some(expected), Some(actual)) if expected != actual => {
                    modified.push(IntegrityChange {
                        path: file_path,
                        expected,
                        actual,
                    });
                }
                (None, Some(_)) => {
                    new_files.push(file_path);
                }
                (Some(_), None) => {
                    missing_files.push(file_path);
                }
                _ => {}
            }
        }
    }

    let output = IntegrityOutput {
        mode: "check".into(),
        checked,
        modified,
        new_files,
        missing_files,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}

fn collect_files(path: &str) -> Vec<String> {
    let p = std::path::Path::new(path);
    if p.is_file() {
        return vec![path.to_string()];
    }
    if p.is_dir() {
        return walkdir::WalkDir::new(path)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().display().to_string())
            .collect();
    }
    vec![]
}
