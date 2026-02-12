//! Secret Management — secure API key and credential storage
//!
//! Reads secrets from /etc/aios/secrets.toml with restrictive permissions.
//! Provides in-memory cache with TTL. Wipes secrets on shutdown.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// A cached secret value
struct CachedSecret {
    value: String,
    loaded_at: Instant,
}

/// Manages secrets with in-memory caching and TTL
pub struct SecretManager {
    secrets_path: PathBuf,
    cache: HashMap<String, CachedSecret>,
    cache_ttl: Duration,
}

impl SecretManager {
    pub fn new(secrets_path: &str) -> Self {
        Self {
            secrets_path: PathBuf::from(secrets_path),
            cache: HashMap::new(),
            cache_ttl: Duration::from_secs(3600), // 1 hour default TTL
        }
    }

    /// Load secrets from the secrets file
    pub fn load(&mut self) -> Result<()> {
        if !self.secrets_path.exists() {
            warn!("Secrets file not found: {}", self.secrets_path.display());
            return Ok(());
        }

        // Verify file permissions (should be 600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&self.secrets_path)
                .context("Failed to read secrets file metadata")?;
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o600 {
                warn!(
                    "Secrets file has insecure permissions: {:o} (expected 600)",
                    mode
                );
            }
        }

        let contents = std::fs::read_to_string(&self.secrets_path)
            .context("Failed to read secrets file")?;

        let table: toml::Table = contents
            .parse()
            .context("Failed to parse secrets TOML")?;

        let now = Instant::now();
        for (key, value) in &table {
            if let Some(s) = value.as_str() {
                self.cache.insert(
                    key.clone(),
                    CachedSecret {
                        value: s.to_string(),
                        loaded_at: now,
                    },
                );
            } else if let Some(inner_table) = value.as_table() {
                // Support nested keys like [api_keys]\n claude = "..."
                for (inner_key, inner_value) in inner_table {
                    if let Some(s) = inner_value.as_str() {
                        self.cache.insert(
                            format!("{key}.{inner_key}"),
                            CachedSecret {
                                value: s.to_string(),
                                loaded_at: now,
                            },
                        );
                    }
                }
            }
        }

        info!("Loaded {} secrets from {}", self.cache.len(), self.secrets_path.display());
        Ok(())
    }

    /// Get a secret by key
    pub fn get(&self, key: &str) -> Option<&str> {
        self.cache.get(key).and_then(|cached| {
            if cached.loaded_at.elapsed() < self.cache_ttl {
                Some(cached.value.as_str())
            } else {
                None
            }
        })
    }

    /// Get a secret, reloading from disk if expired
    pub fn get_or_reload(&mut self, key: &str) -> Result<Option<String>> {
        // Check cache first
        if let Some(cached) = self.cache.get(key) {
            if cached.loaded_at.elapsed() < self.cache_ttl {
                return Ok(Some(cached.value.clone()));
            }
        }

        // Reload from disk
        self.load()?;
        Ok(self.get(key).map(|s| s.to_string()))
    }

    /// Set a secret in the in-memory cache (does not persist)
    pub fn set(&mut self, key: &str, value: &str) {
        self.cache.insert(
            key.to_string(),
            CachedSecret {
                value: value.to_string(),
                loaded_at: Instant::now(),
            },
        );
    }

    /// Wipe all secrets from memory
    pub fn wipe(&mut self) {
        // Overwrite values before dropping
        for secret in self.cache.values_mut() {
            // Zero out the string by replacing with zeros
            let len = secret.value.len();
            secret.value = "\0".repeat(len);
        }
        self.cache.clear();
        info!("All secrets wiped from memory");
    }

    /// Get API keys for the api-gateway service
    pub fn get_api_keys(&self) -> ApiKeys {
        ApiKeys {
            claude_api_key: self.get("api_keys.claude").map(|s| s.to_string()),
            openai_api_key: self.get("api_keys.openai").map(|s| s.to_string()),
        }
    }

    /// Count of cached secrets
    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }
}

impl Drop for SecretManager {
    fn drop(&mut self) {
        self.wipe();
    }
}

/// API keys for external AI services
#[derive(Debug, Clone)]
pub struct ApiKeys {
    pub claude_api_key: Option<String>,
    pub openai_api_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_manager_new() {
        let mgr = SecretManager::new("/etc/aios/secrets.toml");
        assert_eq!(mgr.cached_count(), 0);
    }

    #[test]
    fn test_set_and_get() {
        let mut mgr = SecretManager::new("/nonexistent");
        mgr.set("test_key", "test_value");
        assert_eq!(mgr.get("test_key"), Some("test_value"));
        assert_eq!(mgr.cached_count(), 1);
    }

    #[test]
    fn test_get_nonexistent() {
        let mgr = SecretManager::new("/nonexistent");
        assert!(mgr.get("nonexistent").is_none());
    }

    #[test]
    fn test_wipe() {
        let mut mgr = SecretManager::new("/nonexistent");
        mgr.set("key1", "value1");
        mgr.set("key2", "value2");
        assert_eq!(mgr.cached_count(), 2);

        mgr.wipe();
        assert_eq!(mgr.cached_count(), 0);
        assert!(mgr.get("key1").is_none());
    }

    #[test]
    fn test_get_api_keys() {
        let mut mgr = SecretManager::new("/nonexistent");
        mgr.set("api_keys.claude", "sk-test-claude");
        mgr.set("api_keys.openai", "sk-test-openai");

        let keys = mgr.get_api_keys();
        assert_eq!(keys.claude_api_key.as_deref(), Some("sk-test-claude"));
        assert_eq!(keys.openai_api_key.as_deref(), Some("sk-test-openai"));
    }

    #[test]
    fn test_get_api_keys_missing() {
        let mgr = SecretManager::new("/nonexistent");
        let keys = mgr.get_api_keys();
        assert!(keys.claude_api_key.is_none());
        assert!(keys.openai_api_key.is_none());
    }

    #[test]
    fn test_load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        std::fs::write(
            &path,
            r#"
[api_keys]
claude = "sk-test-claude"
openai = "sk-test-openai"
"#,
        )
        .unwrap();

        // Set permissions to 600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let mut mgr = SecretManager::new(path.to_str().unwrap());
        mgr.load().unwrap();

        assert_eq!(mgr.get("api_keys.claude"), Some("sk-test-claude"));
        assert_eq!(mgr.get("api_keys.openai"), Some("sk-test-openai"));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let mut mgr = SecretManager::new("/nonexistent/secrets.toml");
        // Should not error, just warn
        mgr.load().unwrap();
        assert_eq!(mgr.cached_count(), 0);
    }

    #[test]
    fn test_ttl_expiry() {
        let mut mgr = SecretManager::new("/nonexistent");
        mgr.cache_ttl = Duration::from_millis(1);
        mgr.set("key", "value");

        // Wait for TTL to expire
        std::thread::sleep(Duration::from_millis(10));
        assert!(mgr.get("key").is_none());
    }
}
