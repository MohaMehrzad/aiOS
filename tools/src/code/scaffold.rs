//! code.scaffold — Create project directory structures from templates

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct Input {
    /// Project name
    name: String,
    /// Project type: "rust", "python", "node", "generic"
    #[serde(default = "default_project_type")]
    project_type: String,
    /// Root directory to create the project in
    path: String,
    /// Project description
    #[serde(default)]
    description: String,
}

fn default_project_type() -> String {
    "generic".to_string()
}

#[derive(Serialize)]
struct Output {
    success: bool,
    path: String,
    files_created: Vec<String>,
    project_type: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let project_path = Path::new(&input.path).join(&input.name);
    fs::create_dir_all(&project_path).with_context(|| {
        format!(
            "Failed to create project directory: {}",
            project_path.display()
        )
    })?;

    let mut files_created = Vec::new();

    match input.project_type.as_str() {
        "rust" => {
            scaffold_rust(
                &project_path,
                &input.name,
                &input.description,
                &mut files_created,
            )?;
        }
        "python" => {
            scaffold_python(
                &project_path,
                &input.name,
                &input.description,
                &mut files_created,
            )?;
        }
        "node" => {
            scaffold_node(
                &project_path,
                &input.name,
                &input.description,
                &mut files_created,
            )?;
        }
        _ => {
            scaffold_generic(
                &project_path,
                &input.name,
                &input.description,
                &mut files_created,
            )?;
        }
    }

    // Create .gitignore
    let gitignore_content = match input.project_type.as_str() {
        "rust" => "target/\n*.swp\n.DS_Store\n",
        "python" => "__pycache__/\n*.pyc\n.venv/\ndist/\n*.egg-info/\n.DS_Store\n",
        "node" => "node_modules/\ndist/\n.env\n.DS_Store\n",
        _ => ".DS_Store\n*.swp\n",
    };
    write_file(
        &project_path.join(".gitignore"),
        gitignore_content,
        &mut files_created,
    )?;

    // Create README
    let readme = format!(
        "# {}\n\n{}\n\nCreated by aiOS code.scaffold tool.\n",
        input.name,
        if input.description.is_empty() {
            format!("A {} project.", input.project_type)
        } else {
            input.description.clone()
        }
    );
    write_file(&project_path.join("README.md"), &readme, &mut files_created)?;

    let result = Output {
        success: true,
        path: project_path.to_string_lossy().to_string(),
        files_created,
        project_type: input.project_type,
    };
    serde_json::to_vec(&result).context("Failed to serialize output")
}

fn scaffold_rust(
    path: &Path,
    name: &str,
    description: &str,
    files: &mut Vec<String>,
) -> Result<()> {
    fs::create_dir_all(path.join("src"))?;

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
description = "{description}"

[dependencies]
anyhow = "1"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["full"] }}
"#
    );
    write_file(&path.join("Cargo.toml"), &cargo_toml, files)?;

    let main_rs = r#"use anyhow::Result;

fn main() -> Result<()> {
    println!("Hello from {name}!");
    Ok(())
}
"#
    .replace("{name}", name);
    write_file(&path.join("src/main.rs"), &main_rs, files)?;

    Ok(())
}

fn scaffold_python(
    path: &Path,
    name: &str,
    description: &str,
    files: &mut Vec<String>,
) -> Result<()> {
    let pkg_name = name.replace('-', "_");
    fs::create_dir_all(path.join(&pkg_name))?;
    fs::create_dir_all(path.join("tests"))?;

    let pyproject = format!(
        r#"[project]
name = "{name}"
version = "0.1.0"
description = "{description}"
requires-python = ">=3.12"

[build-system]
requires = ["setuptools>=75.0"]
build-backend = "setuptools.build_meta"
"#
    );
    write_file(&path.join("pyproject.toml"), &pyproject, files)?;

    write_file(
        &path.join(&pkg_name).join("__init__.py"),
        &format!("\"\"\"{}.\"\"\"\n\n__version__ = \"0.1.0\"\n", description),
        files,
    )?;

    write_file(&path.join("tests").join("__init__.py"), "", files)?;

    write_file(
        &path.join("tests").join("test_basic.py"),
        &format!("from {pkg_name} import __version__\n\n\ndef test_version():\n    assert __version__ == \"0.1.0\"\n"),
        files,
    )?;

    Ok(())
}

fn scaffold_node(
    path: &Path,
    name: &str,
    description: &str,
    files: &mut Vec<String>,
) -> Result<()> {
    fs::create_dir_all(path.join("src"))?;

    let package_json = format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "description": "{description}",
  "main": "src/index.js",
  "scripts": {{
    "start": "node src/index.js",
    "test": "echo \"No tests yet\""
  }}
}}
"#
    );
    write_file(&path.join("package.json"), &package_json, files)?;

    let index_js = format!("console.log('Hello from {name}!');\n");
    write_file(&path.join("src/index.js"), &index_js, files)?;

    Ok(())
}

fn scaffold_generic(
    path: &Path,
    _name: &str,
    _description: &str,
    files: &mut Vec<String>,
) -> Result<()> {
    fs::create_dir_all(path.join("src"))?;
    fs::create_dir_all(path.join("docs"))?;
    fs::create_dir_all(path.join("tests"))?;

    write_file(&path.join("src/.gitkeep"), "", files)?;
    write_file(&path.join("docs/.gitkeep"), "", files)?;
    write_file(&path.join("tests/.gitkeep"), "", files)?;

    Ok(())
}

fn write_file(path: &Path, content: &str, files: &mut Vec<String>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("Failed to write: {}", path.display()))?;
    files.push(path.to_string_lossy().to_string());
    Ok(())
}
