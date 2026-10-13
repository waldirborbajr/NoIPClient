use std::net::IpAddr;

use anyhow::Result;
use base64::Engine as _;

use crate::{consts::NOIP_UPDATE_URL, http::agent};

pub(crate) fn update_noip(
    user_agent: &str,
    username: &str,
    password: &str,
    hostname: &str,
    ip: IpAddr,
) -> Result<String> {
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    // IPv4 goes in `myip`, IPv6 in `myipv6` (AAAA records).
    let ip_param = match ip {
        IpAddr::V4(_) => "myip",
        IpAddr::V6(_) => "myipv6",
    };
    let request = agent()
        .get(NOIP_UPDATE_URL)
        .query("hostname", hostname)
        .query(ip_param, &ip.to_string())
        .set("Authorization", &format!("Basic {auth}"))
        .set("User-Agent", user_agent);

    match request.call() {
        Ok(resp) => Ok(resp.into_string()?),
        Err(ureq::Error::Status(_, resp)) => Ok(resp.into_string()?),
        Err(e) => Err(e.into()),
    }
}

// ---------------------------------------------------------------------------
// Outcome classification
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum Outcome {
    Updated,
    Unchanged,
    Retry(String),
    /// Terminal — do not retry.
    Fatal(String),
    /// Rate-limited — set `blocked_until`.
    Abuse,
}

pub(crate) fn classify(body: &str) -> Outcome {
    let code = body.split_whitespace().next().unwrap_or("");
    match code {
        "good" => Outcome::Updated,
        "nochg" => Outcome::Unchanged,
        "badauth" | "badagent" | "nohost" | "notfqdn" => Outcome::Fatal(code.into()),
        "abuse" => Outcome::Abuse,
        "911" => Outcome::Retry(code.into()),
        other => Outcome::Retry(format!("unexpected response: {other:?}")),
    }
}
