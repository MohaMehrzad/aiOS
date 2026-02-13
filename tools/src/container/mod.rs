//! Container orchestration tools (wrapping Podman)

pub mod create;
pub mod exec;
pub mod list;
pub mod logs;
pub mod start;
pub mod stop;

use crate::registry::{make_tool, Registry};

/// Register every container tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "container.create",
        "container",
        "Create a new Podman container from an image",
        vec!["container.manage"],
        "medium",
        false,
        true,
        30000,
    ));

    reg.register_tool(make_tool(
        "container.start",
        "container",
        "Start a stopped container",
        vec!["container.manage"],
        "low",
        true,
        true,
        10000,
    ));

    reg.register_tool(make_tool(
        "container.stop",
        "container",
        "Stop a running container",
        vec!["container.manage"],
        "low",
        true,
        true,
        15000,
    ));

    reg.register_tool(make_tool(
        "container.list",
        "container",
        "List all containers with status and port info",
        vec!["container.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "container.exec",
        "container",
        "Execute a command in a running container",
        vec!["container.manage"],
        "high",
        false,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "container.logs",
        "container",
        "Get container logs",
        vec!["container.read"],
        "low",
        true,
        false,
        10000,
    ));
}
