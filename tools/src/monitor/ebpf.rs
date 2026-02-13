//! monitor.ebpf_trace — Syscall tracing via /proc (fallback when eBPF unavailable)
//!
//! On full Linux with eBPF support, this would use the `aya` crate.
//! This implementation falls back to /proc-based tracing.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct EbpfTraceInput {
    #[serde(default = "default_trace_type")]
    trace_type: String,
    #[serde(default = "default_duration")]
    duration_secs: u64,
    #[serde(default)]
    pid: Option<u32>,
}

fn default_trace_type() -> String {
    "process_spawns".into()
}

fn default_duration() -> u64 {
    5
}

#[derive(Serialize)]
struct EbpfTraceOutput {
    trace_type: String,
    events: Vec<TraceEvent>,
    duration_secs: u64,
    method: String,
}

#[derive(Serialize)]
struct TraceEvent {
    pid: u32,
    event: String,
    details: String,
    timestamp: i64,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: EbpfTraceInput =
        serde_json::from_slice(input).context("Invalid monitor.ebpf_trace input")?;

    let events = match req.trace_type.as_str() {
        "process_spawns" => trace_process_spawns(req.duration_secs),
        "file_opens" => trace_file_opens(req.pid),
        "network_connections" => trace_network_connections(),
        _ => vec![],
    };

    let output = EbpfTraceOutput {
        trace_type: req.trace_type,
        events,
        duration_secs: req.duration_secs,
        method: "proc_fallback".into(),
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}

fn trace_process_spawns(duration_secs: u64) -> Vec<TraceEvent> {
    let mut events = Vec::new();
    let now = chrono::Utc::now().timestamp();

    // Read current processes from /proc
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Ok(pid) = name.parse::<u32>() {
                let cmdline_path = format!("/proc/{pid}/cmdline");
                if let Ok(cmdline) = std::fs::read_to_string(&cmdline_path) {
                    let cmd = cmdline.replace('\0', " ").trim().to_string();
                    if !cmd.is_empty() {
                        // Check start time
                        let stat_path = format!("/proc/{pid}/stat");
                        if let Ok(stat) = std::fs::read_to_string(&stat_path) {
                            let parts: Vec<&str> = stat.split_whitespace().collect();
                            if parts.len() > 21 {
                                events.push(TraceEvent {
                                    pid,
                                    event: "process_running".into(),
                                    details: cmd.chars().take(200).collect(),
                                    timestamp: now,
                                });
                            }
                        }
                    }
                }
                if events.len() >= 50 {
                    break;
                }
            }
        }
    }

    events
}

fn trace_file_opens(pid_filter: Option<u32>) -> Vec<TraceEvent> {
    let mut events = Vec::new();
    let now = chrono::Utc::now().timestamp();

    let pids: Vec<u32> = if let Some(pid) = pid_filter {
        vec![pid]
    } else {
        std::fs::read_dir("/proc")
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().to_str()?.parse().ok())
                    .take(20)
                    .collect()
            })
            .unwrap_or_default()
    };

    for pid in pids {
        let fd_path = format!("/proc/{pid}/fd");
        if let Ok(entries) = std::fs::read_dir(&fd_path) {
            for entry in entries.filter_map(|e| e.ok()).take(20) {
                if let Ok(target) = std::fs::read_link(entry.path()) {
                    let target_str = target.display().to_string();
                    if target_str.starts_with('/') && !target_str.contains("/proc/") {
                        events.push(TraceEvent {
                            pid,
                            event: "file_open".into(),
                            details: target_str,
                            timestamp: now,
                        });
                    }
                }
            }
        }
    }

    events.truncate(50);
    events
}

fn trace_network_connections() -> Vec<TraceEvent> {
    let mut events = Vec::new();
    let now = chrono::Utc::now().timestamp();

    // Parse /proc/net/tcp
    if let Ok(content) = std::fs::read_to_string("/proc/net/tcp") {
        for line in content.lines().skip(1).take(30) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                events.push(TraceEvent {
                    pid: 0,
                    event: "tcp_connection".into(),
                    details: format!("local={} remote={} state={}", parts[1], parts[2], parts[3]),
                    timestamp: now,
                });
            }
        }
    }

    events
}
