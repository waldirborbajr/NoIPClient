use std::time::Duration;

pub(crate) const NOIP_UPDATE_URL: &str = "https://dynupdate.no-ip.com/nic/update";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const APP_NAME: &str = "No-IP Client";
pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Fallback public-IP providers, tried in randomized order per run.
pub(crate) const IP_PROVIDERS: &[&str] = &[
    "https://api.ipify.org",
    "https://icanhazip.com",
    "https://ifconfig.me/ip",
    "https://ipinfo.io/ip",
];

/// No-IP `abuse` backoff: 30 minutes.
pub(crate) const ABUSE_BACKOFF: Duration = Duration::from_secs(30 * 60);
