//! Web connectivity tools — HTTP requests, scraping, webhooks, downloads, and API calls.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod api_call;
pub mod download;
pub mod http_request;
pub mod scrape;
pub mod webhook;

use crate::registry::{make_tool, Registry};

/// Register every web tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "web.http_request",
        "web",
        "Perform an HTTP request (GET, POST, PUT, DELETE) with custom headers, body, and authentication",
        vec!["web.http"],
        "medium",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "web.scrape",
        "web",
        "Fetch a web page and extract text content or specific elements using CSS selectors",
        vec!["web.read"],
        "low",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "web.webhook",
        "web",
        "Send a webhook notification to an external URL with a JSON payload",
        vec!["web.write"],
        "medium",
        false,
        false,
        15000,
    ));

    reg.register_tool(make_tool(
        "web.download",
        "web",
        "Download a file from a URL and save it to a local path",
        vec!["web.http", "fs.write"],
        "medium",
        false,
        true,
        120000,
    ));

    reg.register_tool(make_tool(
        "web.api_call",
        "web",
        "Call an external REST API with structured request and parse JSON response",
        vec!["web.http"],
        "medium",
        true,
        false,
        30000,
    ));
}
