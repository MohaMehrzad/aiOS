//! JSON Schema validation for tool inputs and outputs

#![allow(dead_code)]

use anyhow::{bail, Result};

/// Validate a JSON input against a schema
pub fn validate_input(input: &[u8], schema_bytes: &[u8]) -> Result<()> {
    if schema_bytes.is_empty() {
        return Ok(()); // No schema = no validation
    }

    let input_value: serde_json::Value =
        serde_json::from_slice(input).map_err(|e| anyhow::anyhow!("Invalid JSON input: {e}"))?;
    let schema_value: serde_json::Value = serde_json::from_slice(schema_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid JSON schema: {e}"))?;

    // Use jsonschema crate for validation (0.26+ API)
    let validator = jsonschema::validator_for(&schema_value)
        .map_err(|e| anyhow::anyhow!("Invalid JSON schema: {e}"))?;

    if let Err(error) = validator.validate(&input_value) {
        bail!("Input validation failed: {}", error);
    }

    Ok(())
}

/// Parse JSON input bytes into a serde_json::Value
pub fn parse_input(input: &[u8]) -> Result<serde_json::Value> {
    if input.is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    serde_json::from_slice(input).map_err(|e| anyhow::anyhow!("Invalid JSON input: {e}"))
}

/// Serialize output to JSON bytes
pub fn serialize_output(output: &serde_json::Value) -> Result<Vec<u8>> {
    serde_json::to_vec(output).map_err(|e| anyhow::anyhow!("Failed to serialize output: {e}"))
}
