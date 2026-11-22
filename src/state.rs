use std::{fs, net::IpAddr, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::log::log_warn;

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct StateFile {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) hosts: std::collections::BTreeMap<String, HostState>,
    #[serde(default)]
    pub(crate) last_update_check: Option<u64>,
    #[serde(default)]
    pub(crate) last_notified_release: Option<String>,
    #[serde(default)]
    pub(crate) latest_available_release: Option<String>,
    #[serde(default)]
    pub(crate) latest_available_url: Option<String>,
}

impl StateFile {
    pub(crate) fn load(path: &Path) -> Self {
        let mut sf: StateFile = match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    log_warn(&format!("state file corrupt ({e}); starting fresh"));
                    StateFile::default()
                }
            },
            Err(_) => StateFile::default(),
        };
        sf.version = 1;
        sf
    }

    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        atomic_write(path, data.as_bytes(), 0o600)
    }

    pub(crate) fn host(&mut self, hostname: &str) -> &mut HostState {
        self.hosts.entry(hostname.to_string()).or_default()
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub(crate) struct HostState {
    /// Last IP we successfully registered.
    pub(crate) ip: Option<IpAddr>,
    /// Unix timestamp of last successful No-IP contact (good/nochg).
    pub(crate) last_success: Option<u64>,
    /// Consecutive failed runs.
    pub(crate) failures: u32,
    /// Unix timestamp until which we should not call No-IP (abuse/backoff).
    pub(crate) blocked_until: Option<u64>,
}

pub(crate) fn atomic_write(path: &Path, data: &[u8], _mode: u32) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data).with_context(|| format!("writing {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(_mode));
    }
    fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "noip-state-test-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_gives_empty_state() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        let state = StateFile::load(&path);
        assert!(state.hosts.is_empty());
    }

    #[test]
    fn older_state_without_update_timestamp_remains_loadable() {
        let state: StateFile =
            serde_json::from_str(r#"{"version":1,"hosts":{}}"#).expect("deserialize old state");
        assert_eq!(state.last_update_check, None);
        assert_eq!(state.last_notified_release, None);
        assert_eq!(state.latest_available_release, None);
        assert_eq!(state.latest_available_url, None);
    }

    #[test]
    fn saved_state_round_trips() {
        let path = temp_path("roundtrip");
        let mut state = StateFile::default();
        {
            let host = state.host("home.example.ddns.net");
            host.ip = Some("203.0.113.7".parse::<IpAddr>().unwrap());
            host.failures = 2;
            host.last_success = Some(1_700_000_000);
        }
        state.save(&path).expect("save state");

        let loaded = StateFile::load(&path);
        let host = loaded
            .hosts
            .get("home.example.ddns.net")
            .expect("host present after reload");
        assert_eq!(host.failures, 2);
        assert_eq!(host.last_success, Some(1_700_000_000));
        assert_eq!(host.ip, Some("203.0.113.7".parse::<IpAddr>().unwrap()));

        let _ = std::fs::remove_file(&path);
    }
}
