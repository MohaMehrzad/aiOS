//! Hardware information tools — system hardware details.
//!
//! Each submodule exposes `pub fn execute(input: &[u8]) -> Result<Vec<u8>>`.

pub mod info;

use crate::registry::{make_tool, Registry};

/// Register every hardware tool with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "hw.info",
        "hw",
        "Return system hardware information: CPU model, RAM, GPU, and storage devices",
        vec!["hw.read"],
        "low",
        true,
        false,
        10000,
    ));
}
