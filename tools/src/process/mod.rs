//! Process management tools — list, spawn, kill, info, and signal.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`
//! which deserialises JSON input, performs the operation, and returns JSON output.

pub mod cgroup;
pub mod info;
pub mod kill;
pub mod list;
pub mod signal;
pub mod spawn;

use crate::registry::{make_tool, Registry};

/// Register every process tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "process.list",
        "process",
        "List all running processes with pid, name, cpu, memory, and status",
        vec!["process.read"],
        "low",
        true,
        false,
        10000,
    ));

    reg.register_tool(make_tool(
        "process.spawn",
        "process",
        "Spawn a new process with the given command, arguments, and environment variables",
        vec!["process.execute"],
        "high",
        false,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "process.kill",
        "process",
        "Kill a process by PID with the specified signal number",
        vec!["process.kill"],
        "critical",
        false,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "process.info",
        "process",
        "Get detailed information about a process by PID",
        vec!["process.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "process.signal",
        "process",
        "Send a named signal (e.g. SIGHUP, SIGTERM) to a process",
        vec!["process.signal"],
        "high",
        false,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "process.cgroup",
        "process",
        "Manage cgroup v2 resource limits: create groups, assign PIDs, set CPU/memory/IO limits",
        vec!["process.admin"],
        "high",
        false,
        true,
        10000,
    ));
}
