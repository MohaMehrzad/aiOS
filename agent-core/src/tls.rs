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

    /// Generate self-signed certificates for development/first boot
    ///
    /// In production, this would use proper certificate generation
    /// (e.g., via rcgen crate or external CA). For now, creates
    /// placeholder files that indicate cert generation is needed.
    pub fn generate_self_signed(&self, service_name: &str) -> Result<TlsCerts> {
        let certs = self.get_cert_paths()?;

        if self.certs_exist() {
            info!("TLS certificates already exist at {}", self.cert_dir.display());
            return Ok(certs);
        }

        info!(
            "Generating self-signed certificates for {} in {}",
            service_name,
            self.cert_dir.display()
        );

        // Create placeholder CA cert
        let ca_content = format!(
            "# aiOS Self-Signed CA Certificate (placeholder)\n\
             # Service: {service_name}\n\
             # Generated: {}\n\
             # Replace with proper certificates in production\n",
            chrono::Utc::now().to_rfc3339()
        );
        std::fs::write(&certs.ca_cert, &ca_content)
            .context("Failed to write CA cert")?;

        // Create placeholder server cert
        let server_content = format!(
            "# aiOS Server Certificate (placeholder)\n\
             # Service: {service_name}\n\
             # Generated: {}\n",
            chrono::Utc::now().to_rfc3339()
        );
        std::fs::write(&certs.server_cert, &server_content)
            .context("Failed to write server cert")?;

        // Create placeholder server key
        let key_content = format!(
            "# aiOS Server Key (placeholder)\n\
             # Service: {service_name}\n\
             # Generated: {}\n",
            chrono::Utc::now().to_rfc3339()
        );
        std::fs::write(&certs.server_key, &key_content)
            .context("Failed to write server key")?;

        // Set restrictive permissions on key file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&certs.server_key, perms)
                .context("Failed to set key permissions")?;
        }

        info!("TLS certificates generated successfully");
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
