//! Security tools — check_perms, audit_query, grant, revoke, audit, scan, certs, integrity, rootkits.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod audit;
pub mod audit_query;
pub mod cert_generate;
pub mod cert_rotate;
pub mod check_perms;
pub mod file_integrity;
pub mod grant;
pub mod revoke;
pub mod scan;
pub mod scan_rootkits;

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

    reg.register_tool(make_tool(
        "sec.grant",
        "sec",
        "Grant capabilities to an agent with expiration time",
        vec!["sec.admin"],
        "high",
        false,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "sec.revoke",
        "sec",
        "Revoke capabilities from an agent",
        vec!["sec.admin"],
        "high",
        false,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "sec.audit",
        "sec",
        "Query the audit ledger with time, agent, and tool filters",
        vec!["sec.audit"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "sec.scan",
        "sec",
        "Security scan: check open ports, world-writable files, SUID binaries, and weak permissions",
        vec!["sec.read"],
        "medium",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "sec.cert_generate",
        "sec",
        "Generate X.509 certificates (CA + server) using rcgen",
        vec!["sec.admin", "fs_write"],
        "high",
        false,
        true,
        10000,
    ));

    reg.register_tool(make_tool(
        "sec.cert_rotate",
        "sec",
        "Rotate TLS certificates: backup old, generate new, restart services",
        vec!["sec.admin", "fs_write", "service_manage"],
        "high",
        false,
        true,
        30000,
    ));

    reg.register_tool(make_tool(
        "sec.file_integrity",
        "sec",
        "SHA256 checksum verification of critical files against a baseline database",
        vec!["sec.read"],
        "medium",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "sec.scan_rootkits",
        "sec",
        "Scan for hidden processes, suspicious kernel modules, and scripts in /dev/shm and /tmp",
        vec!["sec.read"],
        "medium",
        true,
        false,
        30000,
    ));
}
