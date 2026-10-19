//! Email delivery through an HTTP email API (Resend-compatible JSON body).

use anyhow::{anyhow, Result};

use crate::{config::EmailConfig, http::agent};

pub(crate) fn send(cfg: &EmailConfig, subject: &str, text: &str) -> Result<()> {
    let body = ureq::json!({
        "from": cfg.from,
        "to": cfg.to,
        "subject": subject,
        "text": text,
    });
    agent()
        .post(&cfg.api_url)
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map(|_| ())
        // Never let the API key appear in logs.
        .map_err(|e| anyhow!(e.to_string().replace(cfg.api_key.as_str(), "***")))
}
