use std::{fs, net::IpAddr, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::log::log_warn;

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct StateFile {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) hosts: std::collections::BTreeMap<String, HostState>,
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
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
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
