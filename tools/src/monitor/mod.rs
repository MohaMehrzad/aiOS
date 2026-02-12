//! System monitoring tools — cpu, memory, disk, network, and logs.
//!
//! Uses `sysctl` and system commands on macOS, or `/proc` on Linux.
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod cpu;
pub mod disk;
pub mod logs;
pub mod memory;
pub mod network;

use crate::registry::{make_tool, Registry};

/// Register every monitor tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "monitor.cpu",
        "monitor",
        "Report current CPU usage percentage, core count, and load averages",
        vec!["monitor.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "monitor.memory",
        "monitor",
        "Report memory usage: total, used, available, and utilisation percentage",
        vec!["monitor.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "monitor.disk",
        "monitor",
        "Report disk usage for the filesystem containing a given path",
        vec!["monitor.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "monitor.network",
        "monitor",
        "Report network I/O statistics for a given interface",
        vec!["monitor.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "monitor.logs",
        "monitor",
        "Read recent system log entries, optionally filtered by service name",
        vec!["monitor.read"],
        "low",
        true,
        false,
        10000,
    ));
}
