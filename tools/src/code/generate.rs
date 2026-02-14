//! code.generate — Generate code based on a description and write to file
//!
//! This tool creates a code file based on a description. When a local AI runtime
//! or API gateway is available, it uses AI to generate the code. Otherwise, it
//! creates a well-structured template.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Deserialize)]
struct Input {
    /// Path to write the generated file
    file_path: String,
    /// Description of what the code should do
    description: String,
    /// Programming language (e.g. "rust", "python", "javascript")
    #[serde(default)]
    language: String,
    /// Create parent directories if needed
    #[serde(default = "default_true")]
    create_dirs: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
struct Output {
    success: bool,
    file_path: String,
    language: String,
    lines: usize,
    generated_by: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Infer language from file extension if not specified
    let language = if input.language.is_empty() {
        infer_language(&input.file_path)
    } else {
        input.language.clone()
    };

    // Create parent directories if needed
    if input.create_dirs {
        if let Some(parent) = std::path::Path::new(&input.file_path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
    }

    // Generate code template (AI-enhanced generation happens via agent layer)
    let code = generate_template(&input.description, &language);
    let lines = code.lines().count();

    fs::write(&input.file_path, &code)
        .with_context(|| format!("Failed to write file: {}", input.file_path))?;

    let result = Output {
        success: true,
        file_path: input.file_path,
        language,
        lines,
        generated_by: "template".to_string(),
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn infer_language(file_path: &str) -> String {
    let ext = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

    match ext.as_str() {
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "go" => "go",
        "java" => "java",
        "c" => "c",
        "cpp" | "cc" | "cxx" => "cpp",
        "rb" => "ruby",
        "sh" | "bash" => "shell",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        _ => "text",
    }
    .to_string()
}

fn generate_template(description: &str, language: &str) -> String {
    match language {
        "rust" => format!(
            r#"//! {description}

use anyhow::Result;

/// TODO: Implement — {description}
pub fn run() -> Result<()> {{
    todo!("Implement: {description}")
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn test_run() {{
        // TODO: Add test for: {description}
    }}
}}
"#
        ),
        "python" => format!(
            r#""""{description}."""


def main() -> None:
    """TODO: Implement — {description}."""
    raise NotImplementedError("{description}")


if __name__ == "__main__":
    main()
"#
        ),
        "javascript" | "typescript" => format!(
            r#"// {description}

/**
 * TODO: Implement — {description}
 */
function main() {{
  throw new Error('Not implemented: {description}');
}}

module.exports = {{ main }};
"#
        ),
        "shell" => format!(
            r#"#!/usr/bin/env bash
# {description}
set -euo pipefail

echo "TODO: Implement — {description}"
"#
        ),
        _ => format!("// {description}\n// TODO: Implement\n"),
    }
}
