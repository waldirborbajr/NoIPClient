use std::{io::Read, net::IpAddr};

use anyhow::{anyhow, Context, Result};

use crate::{consts::IP_PROVIDERS, http::agent, log::log_debug, util::shuffle};

/// Try each provider in a shuffled order until one yields a valid public IP.
pub(crate) fn get_public_ip(explicit_url: Option<&str>, want_v6: bool) -> Result<IpAddr> {
    let mut providers: Vec<&str> = match explicit_url {
        Some(u) => vec![u],
        None => {
            let mut p: Vec<&str> = IP_PROVIDERS.to_vec();
            shuffle(&mut p);
            p
        }
    };
    if providers.is_empty() {
        providers.push(IP_PROVIDERS[0]);
    }

    let mut last_err: Option<anyhow::Error> = None;
    for url in providers {
        match fetch_ip(url, want_v6) {
            Ok(ip) => return Ok(ip),
            Err(e) => {
                log_debug(&format!("IP provider {url} failed: {e}"));
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("no IP providers configured")))
}

pub(crate) fn fetch_ip(url: &str, want_v6: bool) -> Result<IpAddr> {
    let resp = agent().get(url).call()?;
    let mut body = String::new();
    resp.into_reader().take(128).read_to_string(&mut body)?;
    let ip: IpAddr = body
        .trim()
        .parse()
        .with_context(|| format!("parsing {:?} from {url}", body.trim()))?;

    match ip {
        IpAddr::V4(v4) if !want_v6 => {
            if v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
            {
                return Err(anyhow!("non-public IPv4 {v4}"));
            }
            Ok(ip)
        }
        IpAddr::V6(v6) if want_v6 => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return Err(anyhow!("non-public IPv6 {v6}"));
            }
            // Reject link-local fe80::/10 and unique-local fc00::/7.
            let seg = v6.segments()[0];
            if (seg & 0xffc0) == 0xfe80 || (seg & 0xfe00) == 0xfc00 {
                return Err(anyhow!("non-public IPv6 {v6}"));
            }
            Ok(ip)
        }
        _ => Err(anyhow!(
            "provider returned wrong family (wanted v{}, got {ip})",
            if want_v6 { 6 } else { 4 }
        )),
    }
}
