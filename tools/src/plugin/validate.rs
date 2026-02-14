//! Plugin Validation — analyze Python code for dangerous operations
//!
//! Scans plugin code for risky patterns before allowing creation.

use anyhow::Result;
use serde::Serialize;
use tracing::warn;

/// Dangerous patterns and their risk scores
const DANGEROUS_PATTERNS: &[(&str, u32, &str)] = &[
    (
        "os.system(",
        30,
        "Arbitrary command execution via os.system",
    ),
    ("subprocess.call(", 20, "Subprocess execution"),
    ("subprocess.Popen(", 20, "Subprocess execution"),
    ("subprocess.run(", 15, "Subprocess execution"),
    ("eval(", 25, "Dynamic code evaluation"),
    ("exec(", 25, "Dynamic code execution"),
    ("compile(", 15, "Dynamic code compilation"),
    ("__import__(", 20, "Dynamic module import"),
    ("importlib.import_module(", 15, "Dynamic module import"),
    ("open(", 5, "File access (check paths)"),
    ("shutil.rmtree(", 20, "Recursive directory deletion"),
    ("os.remove(", 10, "File deletion"),
    ("os.unlink(", 10, "File deletion"),
    ("os.rmdir(", 10, "Directory deletion"),
    ("socket.socket(", 10, "Raw socket creation"),
    ("ctypes.", 20, "C library access"),
    ("os.chmod(", 10, "Permission modification"),
    ("os.chown(", 10, "Ownership modification"),
    ("os.setuid(", 30, "Privilege escalation"),
    ("os.setgid(", 30, "Privilege escalation"),
];

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub safe: bool,
    pub risk_score: u32,
    pub findings: Vec<ValidationFinding>,
    pub recommendation: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationFinding {
    pub pattern: String,
    pub risk: u32,
    pub description: String,
    pub line_number: usize,
}

/// Validate plugin code for dangerous operations
pub fn validate_plugin_code(code: &str) -> ValidationResult {
    let mut findings = Vec::new();
    let mut total_risk: u32 = 0;

    for (line_num, line) in code.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with('#') {
            continue;
        }

        for (pattern, risk, description) in DANGEROUS_PATTERNS {
            if trimmed.contains(pattern) {
                findings.push(ValidationFinding {
                    pattern: pattern.to_string(),
                    risk: *risk,
                    description: description.to_string(),
                    line_number: line_num + 1,
                });
                total_risk += risk;
            }
        }
    }

    total_risk = total_risk.min(100);

    let safe = total_risk < 70;
    let recommendation = if total_risk == 0 {
        "Code appears safe".to_string()
    } else if total_risk < 30 {
        "Low risk — minor concerns noted".to_string()
    } else if total_risk < 70 {
        "Medium risk — review findings before deployment".to_string()
    } else {
        "High risk — code contains dangerous patterns and should be rejected".to_string()
    };

    if !safe {
        warn!(
            "Plugin validation failed: risk_score={}, findings={}",
            total_risk,
            findings.len()
        );
    }

    ValidationResult {
        safe,
        risk_score: total_risk,
        findings,
        recommendation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_code() {
        let code = r#"
def main(input_data):
    name = input_data.get("name", "world")
    return {"greeting": f"Hello, {name}!"}
"#;
        let result = validate_plugin_code(code);
        assert!(result.safe);
        assert_eq!(result.risk_score, 0);
    }

    #[test]
    fn test_dangerous_code() {
        let code = r#"
import os, subprocess
def main(input_data):
    os.system("rm -rf /")
    eval(input_data["code"])
    exec(input_data["payload"])
    return {}
"#;
        let result = validate_plugin_code(code);
        assert!(!result.safe);
        assert!(result.risk_score >= 70);
    }

    #[test]
    fn test_comments_ignored() {
        let code = r#"
# os.system("this is a comment")
def main(input_data):
    return {}
"#;
        let result = validate_plugin_code(code);
        assert!(result.safe);
        assert_eq!(result.risk_score, 0);
    }
}
