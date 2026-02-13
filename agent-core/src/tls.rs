//! mTLS Certificate Management
//!
//! Generates self-signed CA and service certificates at first boot.
//! Configures gRPC servers/clients with TLS for inter-service security.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

/// TLS certificate paths for a service
#[derive(Debug, Clone)]
pub struct TlsCerts {
    pub ca_cert: PathBuf,
    pub server_cert: PathBuf,
    pub server_key: PathBuf,
}

/// Manages TLS certificates for aiOS services
pub struct TlsManager {
    cert_dir: PathBuf,
}

impl TlsManager {
    pub fn new(cert_dir: &str) -> Self {
        Self {
            cert_dir: PathBuf::from(cert_dir),
        }
    }

    /// Check if certificates already exist
    pub fn certs_exist(&self) -> bool {
        self.cert_dir.join("ca.crt").exists()
            && self.cert_dir.join("server.crt").exists()
            && self.cert_dir.join("server.key").exists()
    }

    /// Get certificate paths (creates directory if needed)
    pub fn get_cert_paths(&self) -> Result<TlsCerts> {
        std::fs::create_dir_all(&self.cert_dir)
            .context("Failed to create cert directory")?;

        Ok(TlsCerts {
            ca_cert: self.cert_dir.join("ca.crt"),
            server_cert: self.cert_dir.join("server.crt"),
            server_key: self.cert_dir.join("server.key"),
        })
    }

    /// Generate real X.509 certificates using rcgen
    ///
    /// Creates a self-signed CA and a server certificate signed by that CA.
    /// SAN entries include localhost and the service name.
    pub fn generate_self_signed(&self, service_name: &str) -> Result<TlsCerts> {
        let certs = self.get_cert_paths()?;

        if self.certs_exist() {
            info!("TLS certificates already exist at {}", self.cert_dir.display());
            return Ok(certs);
        }

        info!(
            "Generating X.509 certificates for {} in {}",
            service_name,
            self.cert_dir.display()
        );

        // Generate CA certificate
        let mut ca_params = rcgen::CertificateParams::new(vec!["aiOS CA".to_string()])
            .context("Failed to create CA params")?;
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        ca_params.not_after = rcgen::date_time_ymd(2034, 12, 31);
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "aiOS Root CA");
        ca_params
            .distinguished_name
            .push(rcgen::DnType::OrganizationName, "aiOS");

        let ca_key = rcgen::KeyPair::generate().context("Failed to generate CA key pair")?;
        let ca_cert_signed = ca_params
            .self_signed(&ca_key)
            .context("Failed to self-sign CA cert")?;

        // Generate server certificate signed by CA
        let mut server_params =
            rcgen::CertificateParams::new(vec![service_name.to_string()])
                .context("Failed to create server params")?;
        server_params.is_ca = rcgen::IsCa::NoCa;
        server_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        server_params.not_after = rcgen::date_time_ymd(2026, 12, 31);
        server_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, service_name);
        server_params.subject_alt_names = vec![
            rcgen::SanType::DnsName("localhost".try_into().unwrap()),
        ];
        // Add service name as SAN if it's a valid DNS name
        if let Ok(dns_name) = service_name.to_string().try_into() {
            server_params
                .subject_alt_names
                .push(rcgen::SanType::DnsName(dns_name));
        }

        let server_key =
            rcgen::KeyPair::generate().context("Failed to generate server key pair")?;
        let server_cert_signed = server_params
            .signed_by(&server_key, &ca_cert_signed, &ca_key)
            .context("Failed to sign server cert")?;

        // Write PEM files
        std::fs::write(&certs.ca_cert, ca_cert_signed.pem())
            .context("Failed to write CA cert")?;
        std::fs::write(&certs.server_cert, server_cert_signed.pem())
            .context("Failed to write server cert")?;
        std::fs::write(&certs.server_key, server_key.serialize_pem())
            .context("Failed to write server key")?;

        // Set restrictive permissions on key file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&certs.server_key, perms)
                .context("Failed to set key permissions")?;
        }

        info!("X.509 certificates generated successfully for {service_name}");
        Ok(certs)
    }

    /// Verify certificate files are readable
    pub fn verify_certs(&self) -> Result<bool> {
        let certs = self.get_cert_paths()?;

        let ca_ok = Path::new(&certs.ca_cert).exists();
        let cert_ok = Path::new(&certs.server_cert).exists();
        let key_ok = Path::new(&certs.server_key).exists();

        Ok(ca_ok && cert_ok && key_ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_manager_new() {
        let mgr = TlsManager::new("/tmp/aios-test-certs");
        assert!(!mgr.certs_exist());
    }

    #[test]
    fn test_generate_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let cert_dir = dir.path().join("certs");
        let mgr = TlsManager::new(cert_dir.to_str().unwrap());

        let certs = mgr.generate_self_signed("test-service").unwrap();
        assert!(certs.ca_cert.exists());
        assert!(certs.server_cert.exists());
        assert!(certs.server_key.exists());

        assert!(mgr.verify_certs().unwrap());
        assert!(mgr.certs_exist());
    }

    #[test]
    fn test_idempotent_generation() {
        let dir = tempfile::tempdir().unwrap();
        let cert_dir = dir.path().join("certs");
        let mgr = TlsManager::new(cert_dir.to_str().unwrap());

        mgr.generate_self_signed("svc1").unwrap();
        // Second call should succeed without overwriting
        mgr.generate_self_signed("svc2").unwrap();
        assert!(mgr.certs_exist());
    }
}
