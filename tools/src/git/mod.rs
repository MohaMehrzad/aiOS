//! Git tools — init, clone, add, commit, push, pull, branch, status, log, diff.
//!
//! Uses the git CLI for operations.

pub mod operations;

use crate::registry::{make_tool, Registry};

/// Register every git tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "git.init",
        "git",
        "Initialize a new git repository at the specified path",
        vec!["git.write"],
        "low",
        false,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "git.clone",
        "git",
        "Clone a remote git repository to a local path",
        vec!["git.write", "net.read"],
        "medium",
        false,
        true,
        120000,
    ));

    reg.register_tool(make_tool(
        "git.add",
        "git",
        "Stage files for commit in a git repository",
        vec!["git.write"],
        "low",
        true,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "git.commit",
        "git",
        "Create a commit with the staged changes and a message",
        vec!["git.write"],
        "low",
        false,
        false,
        10000,
    ));

    reg.register_tool(make_tool(
        "git.push",
        "git",
        "Push local commits to a remote repository",
        vec!["git.write", "net.write"],
        "high",
        false,
        false,
        60000,
    ));

    reg.register_tool(make_tool(
        "git.pull",
        "git",
        "Pull latest changes from a remote repository",
        vec!["git.write", "net.read"],
        "medium",
        false,
        false,
        60000,
    ));

    reg.register_tool(make_tool(
        "git.branch",
        "git",
        "Create, list, or switch branches in a git repository",
        vec!["git.write"],
        "low",
        true,
        true,
        5000,
    ));

    reg.register_tool(make_tool(
        "git.status",
        "git",
        "Show the working tree status of a git repository",
        vec!["git.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "git.log",
        "git",
        "Show commit history of a git repository",
        vec!["git.read"],
        "low",
        true,
        false,
        5000,
    ));

    reg.register_tool(make_tool(
        "git.diff",
        "git",
        "Show changes between commits, working tree, and staging area",
        vec!["git.read"],
        "low",
        true,
        false,
        10000,
    ));
}
