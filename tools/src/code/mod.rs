//! Code generation tools — project scaffolding and AI-powered code generation.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod generate;
pub mod scaffold;

use crate::registry::{make_tool, Registry};

/// Register every code tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "code.scaffold",
        "code",
        "Create a new project from a template with directory structure, config files, and README",
        vec!["fs.write", "code.gen"],
        "medium",
        false,
        true,
        15000,
    ));

    reg.register_tool(make_tool(
        "code.generate",
        "code",
        "Generate source code files based on a description, writing the result to a file",
        vec!["code.gen"],
        "medium",
        false,
        true,
        30000,
    ));
}
