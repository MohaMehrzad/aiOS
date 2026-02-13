//! sec.cert_rotate — Rotate certificates: generate new, backup old, reload service

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CertRotateInput {
    service_name: String,
    #[serde(default = "default_cert_dir")]
    cert_dir: String,
}

fn default_cert_dir() -> String {
    "/var/lib/aios/certs".into()
}

#[derive(Serialize)]
struct CertRotateOutput {
    success: bool,
    backed_up: Vec<String>,
    regenerated: bool,
    service_name: String,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: CertRotateInput =
        serde_json::from_slice(input).context("Invalid sec.cert_rotate input")?;

    let cert_dir = std::path::Path::new(&req.cert_dir);
    let backup_dir = cert_dir.join("backup");
    std::fs::create_dir_all(&backup_dir).context("Failed to create backup directory")?;

    // Backup existing certs
    let mut backed_up = Vec::new();
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    for filename in &["ca.crt", "server.crt", "server.key"] {
        let src = cert_dir.join(filename);
        if src.exists() {
            let dst = backup_dir.join(format!("{}_{}", timestamp, filename));
            std::fs::copy(&src, &dst).context("Failed to backup cert")?;
            backed_up.push(dst.display().to_string());
        }
    }

    // Generate new certificates using cert_generate
    let gen_input = serde_json::json!({
        "service_name": req.service_name,
        "cert_dir": req.cert_dir,
        "validity_years": 2,
    });
    let gen_bytes = serde_json::to_vec(&gen_input)?;
    super::cert_generate::execute(&gen_bytes)?;

    let output = CertRotateOutput {
        success: true,
        backed_up,
        regenerated: true,
        service_name: req.service_name,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}
