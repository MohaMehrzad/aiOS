//! sec.cert_generate — Generate real X.509 certificates using rcgen

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CertGenInput {
    service_name: String,
    #[serde(default = "default_cert_dir")]
    cert_dir: String,
    #[serde(default = "default_validity_years")]
    validity_years: i32,
}

fn default_cert_dir() -> String {
    "/var/lib/aios/certs".into()
}

fn default_validity_years() -> i32 {
    2
}

#[derive(Serialize)]
struct CertGenOutput {
    success: bool,
    ca_cert_path: String,
    server_cert_path: String,
    server_key_path: String,
    expires_year: i32,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let req: CertGenInput =
        serde_json::from_slice(input).context("Invalid sec.cert_generate input")?;

    std::fs::create_dir_all(&req.cert_dir).context("Failed to create cert directory")?;

    let ca_cert_path = format!("{}/ca.crt", req.cert_dir);
    let server_cert_path = format!("{}/server.crt", req.cert_dir);
    let server_key_path = format!("{}/server.key", req.cert_dir);

    // Generate CA
    let mut ca_params = rcgen::CertificateParams::new(vec!["aiOS CA".to_string()])
        .context("Failed to create CA params")?;
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    ca_params.not_after = rcgen::date_time_ymd(2024 + req.validity_years + 8, 12, 31);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "aiOS Root CA");
    ca_params
        .distinguished_name
        .push(rcgen::DnType::OrganizationName, "aiOS");

    let ca_key = rcgen::KeyPair::generate().context("Failed to generate CA key pair")?;
    let ca_cert = ca_params
        .self_signed(&ca_key)
        .context("Failed to self-sign CA cert")?;

    // Generate server cert
    let mut server_params =
        rcgen::CertificateParams::new(vec![req.service_name.clone()])
            .context("Failed to create server params")?;
    server_params.is_ca = rcgen::IsCa::NoCa;
    server_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    server_params.not_after = rcgen::date_time_ymd(2024 + req.validity_years, 12, 31);
    server_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, &req.service_name);
    server_params.subject_alt_names =
        vec![rcgen::SanType::DnsName("localhost".try_into().unwrap())];

    if let Ok(dns_name) = req.service_name.clone().try_into() {
        server_params
            .subject_alt_names
            .push(rcgen::SanType::DnsName(dns_name));
    }

    let server_key = rcgen::KeyPair::generate().context("Failed to generate server key")?;
    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .context("Failed to sign server cert")?;

    // Write PEM files
    std::fs::write(&ca_cert_path, ca_cert.pem()).context("Failed to write CA cert")?;
    std::fs::write(&server_cert_path, server_cert.pem()).context("Failed to write server cert")?;
    std::fs::write(&server_key_path, server_key.serialize_pem())
        .context("Failed to write server key")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&server_key_path, perms).ok();
    }

    let output = CertGenOutput {
        success: true,
        ca_cert_path,
        server_cert_path,
        server_key_path,
        expires_year: 2024 + req.validity_years,
    };
    serde_json::to_vec(&output).context("Failed to serialize output")
}
