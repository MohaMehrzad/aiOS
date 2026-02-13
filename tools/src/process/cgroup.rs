//! process.cgroup — cgroup v2 resource control
//!
//! Set CPU shares, memory limits, and I/O weight for processes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CgroupInput {
    #[serde(default = "default_action")]
    action: String,
    #[serde(default)]
    group_name: String,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    cpu_weight: Option<u32>,
    #[serde(default)]
    memory_max_mb: Option<u64>,
    #[serde(default)]
    io_weight: Option<u32>,
}

fn default_action() -> String {
    "status".into()
}

#[derive(Serialize)]
struct CgroupOutput {
    success: bool,
    action: String,
    group_name: String,
    details: serde_json::Value,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: CgroupInput =
        serde_json::from_slice(input).context("Invalid process.cgroup input")?;

    let cgroup_base = "/sys/fs/cgroup";
    let group_path = if req.group_name.is_empty() {
        format!("{cgroup_base}/aios")
    } else {
        format!("{cgroup_base}/aios/{}", req.group_name)
    };

    let result = match req.action.as_str() {
        "create" => create_cgroup(&group_path, &req),
        "assign" => assign_to_cgroup(&group_path, &req),
        "status" => status_cgroup(&group_path),
        "remove" => remove_cgroup(&group_path),
        _ => {
            let output = CgroupOutput {
                success: false,
                action: req.action.clone(),
                group_name: req.group_name,
                details: serde_json::json!({"error": format!("Unknown action: {}", req.action)}),
            };
            serde_json::to_vec(&output).context("Failed to serialize output")?
        }
    };

    Ok(result)
}

fn create_cgroup(path: &str, req: &CgroupInput) -> Vec<u8> {
    let mut details = serde_json::Map::new();

    match std::fs::create_dir_all(path) {
        Ok(_) => {
            details.insert("created".into(), serde_json::json!(true));

            if let Some(cpu_weight) = req.cpu_weight {
                let cpu_path = format!("{path}/cpu.weight");
                if std::fs::write(&cpu_path, cpu_weight.to_string()).is_ok() {
                    details.insert("cpu_weight".into(), serde_json::json!(cpu_weight));
                }
            }

            if let Some(mem_mb) = req.memory_max_mb {
                let mem_path = format!("{path}/memory.max");
                let mem_bytes = mem_mb * 1024 * 1024;
                if std::fs::write(&mem_path, mem_bytes.to_string()).is_ok() {
                    details.insert("memory_max_mb".into(), serde_json::json!(mem_mb));
                }
            }

            if let Some(io_weight) = req.io_weight {
                let io_path = format!("{path}/io.weight");
                if std::fs::write(&io_path, io_weight.to_string()).is_ok() {
                    details.insert("io_weight".into(), serde_json::json!(io_weight));
                }
            }
        }
        Err(e) => {
            details.insert("error".into(), serde_json::json!(e.to_string()));
        }
    }

    let output = CgroupOutput {
        success: details.contains_key("created"),
        action: "create".into(),
        group_name: req.group_name.clone(),
        details: serde_json::Value::Object(details),
    };
    serde_json::to_vec(&output).unwrap_or_default()
}

fn assign_to_cgroup(path: &str, req: &CgroupInput) -> Vec<u8> {
    let mut details = serde_json::Map::new();
    let success = if let Some(pid) = req.pid {
        let procs_path = format!("{path}/cgroup.procs");
        match std::fs::write(&procs_path, pid.to_string()) {
            Ok(_) => {
                details.insert("pid".into(), serde_json::json!(pid));
                true
            }
            Err(e) => {
                details.insert("error".into(), serde_json::json!(e.to_string()));
                false
            }
        }
    } else {
        details.insert("error".into(), serde_json::json!("No PID specified"));
        false
    };

    let output = CgroupOutput {
        success,
        action: "assign".into(),
        group_name: req.group_name.clone(),
        details: serde_json::Value::Object(details),
    };
    serde_json::to_vec(&output).unwrap_or_default()
}

fn status_cgroup(path: &str) -> Vec<u8> {
    let mut details = serde_json::Map::new();

    let exists = std::path::Path::new(path).exists();
    details.insert("exists".into(), serde_json::json!(exists));

    if exists {
        if let Ok(content) = std::fs::read_to_string(format!("{path}/cpu.weight")) {
            details.insert(
                "cpu_weight".into(),
                serde_json::json!(content.trim()),
            );
        }
        if let Ok(content) = std::fs::read_to_string(format!("{path}/memory.max")) {
            details.insert(
                "memory_max".into(),
                serde_json::json!(content.trim()),
            );
        }
        if let Ok(content) = std::fs::read_to_string(format!("{path}/memory.current")) {
            details.insert(
                "memory_current".into(),
                serde_json::json!(content.trim()),
            );
        }
        if let Ok(content) = std::fs::read_to_string(format!("{path}/cgroup.procs")) {
            let pids: Vec<&str> = content.lines().collect();
            details.insert("process_count".into(), serde_json::json!(pids.len()));
        }
    }

    let output = CgroupOutput {
        success: exists,
        action: "status".into(),
        group_name: String::new(),
        details: serde_json::Value::Object(details),
    };
    serde_json::to_vec(&output).unwrap_or_default()
}

fn remove_cgroup(path: &str) -> Vec<u8> {
    let success = std::fs::remove_dir(path).is_ok();
    let output = CgroupOutput {
        success,
        action: "remove".into(),
        group_name: String::new(),
        details: serde_json::json!({"removed": success}),
    };
    serde_json::to_vec(&output).unwrap_or_default()
}
