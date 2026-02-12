//! Network tools — interfaces, ping, dns, http_get, and port_scan.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod dns;
pub mod http_get;
pub mod interfaces;
pub mod ping;
pub mod port_scan;

use crate::registry::{make_tool, Registry};

/// Register every network tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "net.interfaces",
        "net",
        "List all network interfaces with name, IP address, MAC address, and status",
        vec!["net.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "net.ping",
        "net",
        "Ping a remote host and return success status and latency",
        vec!["net.read"],
        "low",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "net.dns",
        "net",
        "Perform a DNS lookup for a hostname and return resolved addresses",
        vec!["net.read"],
        "low",
        true,
        false,
        10000,
    ));

    reg.register_tool(make_tool(
        "net.http_get",
        "net",
        "Perform an HTTP GET request and return the status code and response body",
        vec!["net.http"],
        "medium",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "net.port_scan",
        "net",
        "Check whether a specific TCP port is open on a given host",
        vec!["net.read"],
        "medium",
        true,
        false,
        10000,
    ));
}
