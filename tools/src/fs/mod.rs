//! Filesystem tools — read, write, delete, list, stat, mkdir, move, copy,
//! chmod, chown, symlink, search, and disk_usage.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`
//! which deserialises JSON input, performs the operation, and returns JSON output.

pub mod chmod;
pub mod chown;
pub mod copy;
pub mod delete;
pub mod disk_usage;
pub mod list;
pub mod mkdir;
pub mod move_file;
pub mod read;
pub mod search;
pub mod stat;
pub mod symlink;
pub mod write;

use crate::registry::{make_tool, Registry};

/// Register every filesystem tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "fs.read",
        "fs",
        "Read file contents and return them as a UTF-8 string",
        vec!["fs.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "fs.write",
        "fs",
        "Write content to a file, creating it if it does not exist. Backs up the original first.",
        vec!["fs.write"],
        "medium",
        false,
        true,
        10000,
    ));

    reg.register_tool(make_tool(
        "fs.delete",
        "fs",
        "Delete a file or directory. Supports recursive deletion.",
        vec!["fs.delete"],
        "high",
        false,
        false,
        10000,
    ));

    reg.register_tool(make_tool(
        "fs.list",
        "fs",
        "List directory contents with name, type, size, and last-modified timestamp",
        vec!["fs.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "fs.stat",
        "fs",
        "Return file metadata: size, permissions, timestamps, and type flags",
        vec!["fs.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "fs.mkdir",
        "fs",
        "Create a directory. Supports recursive creation of parent directories.",
        vec!["fs.write"],
        "low",
        true,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "fs.move",
        "fs",
        "Move or rename a file or directory",
        vec!["fs.write", "fs.delete"],
        "medium",
        false,
        true,
        10000,
    ));

    reg.register_tool(make_tool(
        "fs.copy",
        "fs",
        "Copy a file to a new location",
        vec!["fs.read", "fs.write"],
        "medium",
        false,
        true,
        30000,
    ));

    reg.register_tool(make_tool(
        "fs.chmod",
        "fs",
        "Change file permissions using an octal mode string",
        vec!["fs.write"],
        "high",
        true,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "fs.chown",
        "fs",
        "Change file ownership (uid / gid)",
        vec!["fs.admin"],
        "critical",
        true,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "fs.symlink",
        "fs",
        "Create a symbolic link pointing to a target path",
        vec!["fs.write"],
        "medium",
        false,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "fs.search",
        "fs",
        "Search for files matching a glob pattern under a directory tree",
        vec!["fs.read"],
        "low",
        true,
        false,
        30000,
    ));

    reg.register_tool(make_tool(
        "fs.disk_usage",
        "fs",
        "Report disk usage (total, used, available) for the filesystem containing a path",
        vec!["fs.read"],
        "low",
        true,
        false,
        5000,
    ));
}
