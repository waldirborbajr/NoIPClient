use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::{
    config::Config,
    consts::{APP_VERSION, GITHUB_LATEST_RELEASE_URL, UPDATE_CHECK_INTERVAL_SECS},
    http,
    log::{log_info, log_warn},
    notify,
    state::StateFile,
    util::now_secs,
};

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
}

#[derive(Clone)]
pub(crate) struct UpdateInfo {
    pub(crate) tag_name: String,
    pub(crate) html_url: String,
}

pub(crate) fn check_for_updates(cfg: &Config, state: &mut StateFile) -> Option<UpdateInfo> {
    let now = now_secs();
    if !check_is_due(state.last_update_check, now) {
        return cached_update(state);
    }
    state.last_update_check = Some(now);

    match latest_release() {
        Ok(release) => match is_newer_version(&release.tag_name, APP_VERSION) {
            Ok(true) => {
                state.latest_available_release = Some(release.tag_name.clone());
                state.latest_available_url = Some(release.html_url.clone());
                if !should_notify_release(state.last_notified_release.as_deref(), &release.tag_name)
                {
                    return Some(UpdateInfo {
                        tag_name: release.tag_name,
                        html_url: release.html_url,
                    });
                }
                let message = format!(
                    "Update available: {} (current version: {}). Download: {}",
                    release.tag_name, APP_VERSION, release.html_url
                );
                log_info(&message);
                let delivered =
                    match notify::send(cfg, "[NoIPClient] Update available", &message, false) {
                        Ok(()) => true,
                        Err(e) => {
                            log_warn(&format!("update notification failed: {e:#}"));
                            false
                        }
                    };
                if delivered {
                    state.last_notified_release = Some(release.tag_name.clone());
                }
                Some(UpdateInfo {
                    tag_name: release.tag_name,
                    html_url: release.html_url,
                })
            }
            Ok(false) => {
                state.latest_available_release = None;
                state.latest_available_url = None;
                None
            }
            Err(e) => {
                log_warn(&format!("could not compare latest release version: {e:#}"));
                cached_update(state)
            }
        },
        Err(e) => {
            log_warn(&format!("could not check for updates: {e:#}"));
            cached_update(state)
        }
    }
}

fn cached_update(state: &StateFile) -> Option<UpdateInfo> {
    match (
        state.latest_available_release.as_ref(),
        state.latest_available_url.as_ref(),
    ) {
        (Some(tag_name), Some(html_url)) => Some(UpdateInfo {
            tag_name: tag_name.clone(),
            html_url: html_url.clone(),
        }),
        _ => None,
    }
}

fn latest_release() -> Result<LatestRelease> {
    http::agent()
        .get(GITHUB_LATEST_RELEASE_URL)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &format!("noipclient/{APP_VERSION}"))
        .call()
        .context("requesting the latest GitHub release")?
        .into_json()
        .context("parsing the latest GitHub release response")
}

fn check_is_due(last_check: Option<u64>, now: u64) -> bool {
    last_check.is_none_or(|last| now.saturating_sub(last) >= UPDATE_CHECK_INTERVAL_SECS)
}

fn should_notify_release(last_notified: Option<&str>, latest: &str) -> bool {
    last_notified != Some(latest)
}

fn is_newer_version(latest: &str, current: &str) -> Result<bool> {
    let latest = parse_version(latest)?;
    let current = parse_version(current)?;
    Ok(latest > current)
}

fn parse_version(version: &str) -> Result<[u64; 3]> {
    let version = version.trim();
    let version = version.strip_prefix('v').unwrap_or(version);
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow!("expected a three-part version, got {version:?}"));
    }
    let parsed = parts
        .iter()
        .map(|part| {
            part.parse::<u64>()
                .with_context(|| format!("invalid version component {part:?}"))
        })
        .collect::<Result<Vec<_>>>()?;
    parsed
        .try_into()
        .map_err(|_| anyhow!("expected a three-part version, got {version:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_check_is_limited_to_once_per_interval() {
        assert!(check_is_due(None, 100));
        assert!(!check_is_due(
            Some(100),
            100 + UPDATE_CHECK_INTERVAL_SECS - 1
        ));
        assert!(check_is_due(Some(100), 100 + UPDATE_CHECK_INTERVAL_SECS));
    }

    #[test]
    fn release_is_not_notified_twice() {
        assert!(should_notify_release(None, "v1.2.3"));
        assert!(!should_notify_release(Some("v1.2.3"), "v1.2.3"));
        assert!(should_notify_release(Some("v1.2.3"), "v1.2.4"));
    }

    #[test]
    fn compares_release_versions_numerically() {
        assert!(is_newer_version("v1.10.0", "1.9.9").unwrap());
        assert!(!is_newer_version("v1.2.3", "1.2.3").unwrap());
        assert!(!is_newer_version("v1.2.2", "1.2.3").unwrap());
    }

    #[test]
    fn rejects_malformed_release_versions() {
        assert!(is_newer_version("v1.2", APP_VERSION).is_err());
        assert!(is_newer_version("v1.x.3", APP_VERSION).is_err());
    }
}
