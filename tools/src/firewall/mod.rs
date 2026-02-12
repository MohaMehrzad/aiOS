//! Firewall management tools — rules, add_rule, delete_rule.
//!
//! On macOS, uses `pfctl` (Packet Filter). On Linux, uses `nft` (nftables).
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod add_rule;
pub mod delete_rule;
pub mod rules;

use crate::registry::{make_tool, Registry};

/// Register every firewall tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "firewall.rules",
        "firewall",
        "List all current firewall rules",
        vec!["firewall.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "firewall.add_rule",
        "firewall",
        "Add a new firewall rule to a chain with the specified action",
        vec!["firewall.manage"],
        "critical",
        false,
        true,
        10000,
    ));

    reg.register_tool(make_tool(
        "firewall.delete_rule",
        "firewall",
        "Delete a firewall rule by chain and index",
        vec!["firewall.manage"],
        "critical",
        false,
        false,
        10000,
    ));
}
