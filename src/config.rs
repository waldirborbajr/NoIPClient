use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::consts::IP_PROVIDERS;

#[derive(Debug, Deserialize)]
pub(crate) struct Config {
    #[serde(default = "default_interval")]
    pub(crate) check_interval_secs: u64,
    #[serde(default = "default_ip_check")]
    pub(crate) ip_check_url: String,
    #[serde(default = "default_state_file")]
    pub(crate) state_file: PathBuf,
    pub(crate) prometheus_file: Option<PathBuf>,
    /// Accepts humantime-ish strings like "30s", "10m", "5h", "2d".
    #[serde(default = "default_force_after")]
    pub(crate) force_update_after: String,
    #[serde(default)]
    pub(crate) silent_on_unchanged: bool,

    pub(crate) noip: NoipConfig,
    #[serde(default)]
    pub(crate) telegram: Option<TelegramConfig>,
    pub(crate) hosts: Vec<HostConfig>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NoipConfig {
    pub(crate) user_agent: String,
    #[serde(default)]
    pub(crate) username: Option<String>,
    #[serde(default)]
    pub(crate) password: Option<String>,
    #[serde(default)]
    pub(crate) credentials_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct TelegramConfig {
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) chat_ids: Vec<String>,
    #[serde(default)]
    pub(crate) silent_on_unchanged: bool,
}

impl TelegramConfig {
    pub(crate) fn enabled(&self) -> bool {
        self.token.as_deref().is_some_and(|t| !t.is_empty()) && !self.chat_ids.is_empty()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct HostConfig {
    pub(crate) hostname: String,
    #[serde(default = "default_ip_version")]
    pub(crate) ip_version: u8,
    #[serde(default)]
    pub(crate) ip_check_url: Option<String>,
    #[serde(default)]
    pub(crate) silent_on_unchanged: Option<bool>,
}

pub(crate) fn default_interval() -> u64 {
    300
}
pub(crate) fn default_ip_check() -> String {
    IP_PROVIDERS[0].into()
}
pub(crate) fn default_state_file() -> PathBuf {
    "noip-state.json".into()
}
pub(crate) fn default_force_after() -> String {
    "25d".into()
}
pub(crate) fn default_ip_version() -> u8 {
    4
}

impl Config {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        if cfg.hosts.is_empty() {
            return Err(anyhow!("config has no [[hosts]] entries"));
        }
        for h in &cfg.hosts {
            if h.ip_version != 4 && h.ip_version != 6 {
                return Err(anyhow!("host {}: ip_version must be 4 or 6", h.hostname));
            }
        }
        Ok(cfg)
    }

    pub(crate) fn credentials(&self) -> Result<(String, String)> {
        if let (Some(u), Some(p)) = (&self.noip.username, &self.noip.password) {
            return Ok((u.clone(), p.clone()));
        }
        if let Some(path) = &self.noip.credentials_file {
            let text = fs::read_to_string(path)
                .with_context(|| format!("reading credentials {}", path.display()))?;
            let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
            let user = lines
                .next()
                .ok_or_else(|| anyhow!("credentials file missing username"))?;
            let pass = lines
                .next()
                .ok_or_else(|| anyhow!("credentials file missing password"))?;
            return Ok((user.to_string(), pass.to_string()));
        }
        Err(anyhow!(
            "no No-IP credentials: set noip.username/password or noip.credentials_file"
        ))
    }

    pub(crate) fn force_update_after(&self) -> Result<u64> {
        parse_duration(&self.force_update_after)
            .ok_or_else(|| anyhow!("bad force_update_after: {:?}", self.force_update_after))
    }
}

/// Parse "30s", "10m", "5h", "2d" (bare integers mean seconds).
pub(crate) fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.chars().last()? {
        's' => (&s[..s.len() - 1], 1),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 3_600),
        'd' => (&s[..s.len() - 1], 86_400),
        c if c.is_ascii_digit() => (s, 1),
        _ => return None,
    };
    num.trim().parse::<u64>().ok().map(|n| n * mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_seconds() {
        assert_eq!(parse_duration("300"), Some(300));
    }

    #[test]
    fn parses_unit_suffixes() {
        assert_eq!(parse_duration("30s"), Some(30));
        assert_eq!(parse_duration("5m"), Some(300));
        assert_eq!(parse_duration("2h"), Some(7_200));
        assert_eq!(parse_duration("1d"), Some(86_400));
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(parse_duration(" 30s "), Some(30));
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration("10x"), None);
        assert_eq!(parse_duration("m"), None);
    }
}
