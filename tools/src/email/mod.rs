//! Email tools — send emails via SMTP.
//!
//! Uses SMTP configuration from /var/lib/aios/config/smtp.json.

pub mod send;

use crate::registry::{make_tool, Registry};

/// Register email tools with the registry.
pub fn register_tools(reg: &mut Registry) {
    reg.register_tool(make_tool(
        "email.send",
        "email",
        "Send an email via SMTP. Input: {\"to\": \"recipient@email.com\", \"subject\": \"Subject line\", \"body\": \"Email body text\"}. Optional: from, reply_to, cc.",
        vec!["email_send"],
        "medium",
        false,
        false,
        30000,
    ));
}
