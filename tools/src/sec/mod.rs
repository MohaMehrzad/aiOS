//! Security tools — check_perms, audit_query.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod audit_query;
pub mod check_perms;

use crate::registry::{make_tool, Registry};

/// Register every security tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "sec.check_perms",
        "sec",
        "Check file permissions and ownership, reporting owner, group, mode, and world-writability",
        vec!["sec.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "sec.audit_query",
        "sec",
        "Query the audit log for recent tool executions, optionally filtered by tool name",
        vec!["sec.audit"],
        "low",
        true,
        false,
        5000,
    ));
}
