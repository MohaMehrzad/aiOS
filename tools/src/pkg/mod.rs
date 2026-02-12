//! Package management tools — install, remove, search, update, list_installed.
//!
//! On macOS, uses `brew` (Homebrew). On Linux, detects and uses `apt` or `dnf`.
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod install;
pub mod list_installed;
pub mod remove;
pub mod search;
pub mod update;

use crate::registry::{make_tool, Registry};

/// Register every package management tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "pkg.install",
        "pkg",
        "Install a package by name and return the installed version",
        vec!["pkg.manage"],
        "high",
        false,
        true,
        120000,
    ));

    reg.register_tool(make_tool(
        "pkg.remove",
        "pkg",
        "Remove an installed package by name",
        vec!["pkg.manage"],
        "high",
        false,
        false,
        60000,
    ));

    reg.register_tool(make_tool(
        "pkg.search",
        "pkg",
        "Search available packages matching a query string",
        vec!["pkg.read"],
        "low",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "pkg.update",
        "pkg",
        "Update all installed packages to their latest versions",
        vec!["pkg.manage"],
        "high",
        false,
        false,
        300000,
    ));

    reg.register_tool(make_tool(
        "pkg.list_installed",
        "pkg",
        "List all currently installed packages with name and version",
        vec!["pkg.read"],
        "low",
        true,
        false,
        15000,
    ));
}
