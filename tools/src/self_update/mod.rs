//! Self-update tools — inspect, update, rebuild, and health check.
//!
//! These tools allow aiOS to inspect and update its own source code,
//! rebuild components, and check system health.

pub mod inspect;
pub mod update;

use crate::registry::{make_tool, Registry};

/// Register every self-update tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "self.inspect",
        "self",
        "Inspect aiOS source code, version, capabilities, and configuration",
        vec!["self.read"],
        "low",
        true,
        false,
        10000,
    ));

    reg.register_tool(make_tool(
        "self.health",
        "self",
        "Check aiOS system health: service status, resource usage, component connectivity",
        vec!["self.read"],
        "low",
        true,
        false,
        15000,
    ));

    reg.register_tool(make_tool(
        "self.update",
        "self",
        "Pull latest aiOS source code from the repository and apply updates",
        vec!["self.update"],
        "critical",
        false,
        false,
        120000,
    ));

    reg.register_tool(make_tool(
        "self.rebuild",
        "self",
        "Rebuild aiOS components from source after an update",
        vec!["self.update"],
        "critical",
        false,
        false,
        300000,
    ));
}
