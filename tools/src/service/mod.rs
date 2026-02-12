//! Service management tools — list, start, stop, restart, and status.
//!
//! On macOS, services are managed through `launchctl`. On Linux, `systemctl`
//! is used. Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod list;
pub mod restart;
pub mod start;
pub mod status;
pub mod stop;

use crate::registry::{make_tool, Registry};

/// Register every service tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "service.list",
        "service",
        "List all system services with name, status, and pid",
        vec!["service.read"],
        "low",
        true,
        false,
        10000,
    ));

    reg.register_tool(make_tool(
        "service.start",
        "service",
        "Start a system service by name",
        vec!["service.manage"],
        "high",
        false,
        true,
        15000,
    ));

    reg.register_tool(make_tool(
        "service.stop",
        "service",
        "Stop a running system service by name",
        vec!["service.manage"],
        "high",
        false,
        true,
        15000,
    ));

    reg.register_tool(make_tool(
        "service.restart",
        "service",
        "Restart a system service by name (stop then start)",
        vec!["service.manage"],
        "high",
        false,
        true,
        30000,
    ));

    reg.register_tool(make_tool(
        "service.status",
        "service",
        "Get the detailed status of a system service by name",
        vec!["service.read"],
        "low",
        true,
        false,
        5000,
    ));
}
