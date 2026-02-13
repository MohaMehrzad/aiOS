//! monitor.fs_watch — Filesystem event monitoring
//!
//! Uses polling-based detection (checks file modification times).
//! On Linux with inotify, could be upgraded to real-time events.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct FsWatchInput {
    path: String,
    #[serde(default = "default_recursive")]
    recursive: bool,
    #[serde(default)]
    since_timestamp: Option<i64>,
}

fn default_recursive() -> bool {
    true
}

#[derive(Serialize)]
struct FsWatchOutput {
    path: String,
    events: Vec<FsEvent>,
    total_events: usize,
}

#[derive(Serialize)]
struct FsEvent {
    path: String,
    event_type: String,
    modified_at: i64,
    size: u64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: FsWatchInput =
        serde_json::from_slice(input).context("Invalid monitor.fs_watch input")?;

    let since = req.since_timestamp.unwrap_or(0);
    let mut events = Vec::new();

    let walker = if req.recursive {
        walkdir::WalkDir::new(&req.path).max_depth(5)
    } else {
        walkdir::WalkDir::new(&req.path).max_depth(1)
    };

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            if modified > since {
                events.push(FsEvent {
                    path: entry.path().display().to_string(),
                    event_type: "modified".into(),
                    modified_at: modified,
                    size: metadata.len(),
                });
            }
        }

        if events.len() >= 100 {
            break;
        }
    }

    // Sort by modification time descending
    events.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

    let total_events = events.len();
    let output = FsWatchOutput {
        path: req.path,
        events,
        total_events,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}
