use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::{
    config::{Config, HostConfig},
    consts::ABUSE_BACKOFF,
    ip::get_public_ip,
    log::{log_info, log_warn},
    noip::{classify, update_noip, Outcome},
    state::HostState,
    util::now_secs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunResult {
    Changed,
    Unchanged,
    Forced,
    Failure,
    Blocked,
    DryRun,
}

pub(crate) struct RunContext<'a> {
    pub(crate) cfg: &'a Config,
    pub(crate) username: &'a str,
    pub(crate) password: &'a str,
    pub(crate) force: bool,
    pub(crate) dry_run: bool,
    pub(crate) force_after: u64,
}

pub(crate) fn run_host(
    ctx: &RunContext<'_>,
    host: &HostConfig,
    st: &mut HostState,
) -> Result<RunResult> {
    // Abuse backoff gate.
    if let Some(until) = st.blocked_until {
        if now_secs() < until {
            log_warn(&format!(
                "{}: blocked for {} more seconds",
                host.hostname,
                until - now_secs()
            ));
            return Ok(RunResult::Blocked);
        } else {
            st.blocked_until = None;
        }
    }

    let want_v6 = host.ip_version == 6;
    let current = get_public_ip(
        host.ip_check_url
            .as_deref()
            .or(Some(ctx.cfg.ip_check_url.as_str())),
        want_v6,
    )?;

    let needs_force = match st.last_success {
        Some(t) => now_secs().saturating_sub(t) >= ctx.force_after,
        None => true,
    };
    let ip_changed = st.ip != Some(current);

    if !ctx.force && !needs_force && !ip_changed {
        st.failures = 0;
        return Ok(RunResult::Unchanged);
    }

    if ctx.dry_run {
        log_info(&format!(
            "{}: dry-run — would update to {current} (forced={}, changed={})",
            host.hostname, needs_force, ip_changed
        ));
        return Ok(RunResult::DryRun);
    }

    // Bounded retries for transient failures only.
    let mut attempt = 0u32;
    let mut delay = Duration::from_secs(2);
    let body = loop {
        attempt += 1;
        match update_noip(
            &ctx.cfg.noip.user_agent,
            ctx.username,
            ctx.password,
            &host.hostname,
            current,
        ) {
            Ok(body) => match classify(&body) {
                Outcome::Retry(reason) if attempt < 3 => {
                    log_warn(&format!(
                        "{}: retryable response ({reason}), attempt {attempt}/3",
                        host.hostname
                    ));
                    std::thread::sleep(delay);
                    delay *= 2;
                    continue;
                }
                _ => break body,
            },
            Err(e) if attempt < 3 => {
                log_warn(&format!(
                    "{}: network error ({e}), attempt {attempt}/3",
                    host.hostname
                ));
                std::thread::sleep(delay);
                delay *= 2;
                continue;
            }
            Err(e) => return Err(e),
        }
    };

    match classify(&body) {
        Outcome::Updated => {
            st.ip = Some(current);
            st.last_success = Some(now_secs());
            st.failures = 0;
            Ok(if ip_changed {
                RunResult::Changed
            } else {
                RunResult::Forced
            })
        }
        Outcome::Unchanged => {
            // No-IP says nothing to do; trust it over our local view.
            st.ip = Some(current);
            st.last_success = Some(now_secs());
            st.failures = 0;
            Ok(RunResult::Unchanged)
        }
        Outcome::Abuse => {
            st.blocked_until = Some(now_secs() + ABUSE_BACKOFF.as_secs());
            Err(anyhow!("No-IP says 'abuse' — backing off 30 minutes"))
        }
        Outcome::Fatal(code) => Err(anyhow!("No-IP rejected the update: {code}")),
        Outcome::Retry(reason) => Err(anyhow!("No-IP temporary error after retries: {reason}")),
    }
}
