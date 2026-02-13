//! email.send — Send an email via SMTP
//!
//! Reads SMTP configuration from /var/lib/aios/config/smtp.json

use anyhow::{Context, Result};
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::{Deserialize, Serialize};

/// SMTP configuration loaded from disk
#[derive(Deserialize)]
struct SmtpConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
    from_address: String,
    #[serde(default = "default_from_name")]
    from_name: String,
    #[serde(default = "default_true")]
    ssl: bool,
}

fn default_from_name() -> String {
    "aiOS Agent".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct Input {
    /// Recipient email address
    to: String,
    /// Email subject line
    subject: String,
    /// Email body (plain text)
    body: String,
    /// Optional sender override (defaults to SMTP config from_address)
    #[serde(default)]
    from: String,
    /// Optional reply-to address
    #[serde(default)]
    reply_to: String,
    /// Optional CC addresses (comma-separated)
    #[serde(default)]
    cc: String,
}

#[derive(Serialize)]
struct Output {
    success: bool,
    message: String,
    from: String,
    to: String,
}

const SMTP_CONFIG_PATH: &str = "/var/lib/aios/config/smtp.json";

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    // Load SMTP config
    let config_data =
        std::fs::read_to_string(SMTP_CONFIG_PATH).context("SMTP config not found. Create /var/lib/aios/config/smtp.json with host, port, username, password, from_address fields.")?;
    let config: SmtpConfig =
        serde_json::from_str(&config_data).context("Invalid SMTP config JSON")?;

    // Build from address
    let from_mailbox: Mailbox = if input.from.is_empty() {
        Mailbox::new(
            Some(config.from_name.clone()),
            config
                .from_address
                .parse()
                .context("Invalid from_address in SMTP config")?,
        )
    } else {
        input.from.parse().context("Invalid 'from' address")?
    };

    // Build message
    let mut builder = Message::builder()
        .from(from_mailbox)
        .to(input
            .to
            .parse()
            .context("Invalid 'to' address")?)
        .subject(&input.subject);

    if !input.reply_to.is_empty() {
        builder = builder.reply_to(
            input
                .reply_to
                .parse()
                .context("Invalid 'reply_to' address")?,
        );
    }

    if !input.cc.is_empty() {
        for addr in input.cc.split(',') {
            let addr = addr.trim();
            if !addr.is_empty() {
                builder = builder.cc(addr.parse().context("Invalid 'cc' address")?);
            }
        }
    }

    let email = builder
        .body(input.body.clone())
        .context("Failed to build email message")?;

    // Create SMTP transport with credentials
    let creds = Credentials::new(config.username.clone(), config.password.clone());

    let transport = if config.ssl && config.port == 465 {
        // Implicit TLS (SMTPS on port 465)
        SmtpTransport::relay(&config.host)
            .context("Failed to create SMTP relay")?
            .port(config.port)
            .credentials(creds)
            .build()
    } else {
        // STARTTLS (port 587)
        SmtpTransport::starttls_relay(&config.host)
            .context("Failed to create STARTTLS relay")?
            .port(config.port)
            .credentials(creds)
            .build()
    };

    // Send the email
    let response = transport.send(&email).context("SMTP send failed")?;

    let output = Output {
        success: response.is_positive(),
        message: format!(
            "Email sent to {}. Server response: {}",
            input.to,
            response
                .message()
                .collect::<Vec<&str>>()
                .join(" ")
        ),
        from: config.from_address,
        to: input.to,
    };

    serde_json::to_vec(&output).context("Failed to serialize output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_input() {
        let result = execute(b"{}");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_config() {
        let input = serde_json::to_vec(&serde_json::json!({
            "to": "test@example.com",
            "subject": "Test",
            "body": "Hello"
        }))
        .unwrap();
        // Will fail because SMTP config doesn't exist in test environment
        let result = execute(&input);
        assert!(result.is_err());
    }
}
