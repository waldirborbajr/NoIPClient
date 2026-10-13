use std::path::Path;

use anyhow::Result;

use crate::{
    state::{atomic_write, StateFile},
    util::now_secs,
};

pub(crate) fn write_prometheus(path: Option<&Path>, state: &StateFile) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let mut out = String::new();
    let now = now_secs();
    for (host, st) in &state.hosts {
        let label = sanitize_label(host);
        let last = st.last_success.unwrap_or(0);
        out.push_str(&format!(
            "noip_last_success_timestamp{{host=\"{label}\"}} {last}\n"
        ));
        out.push_str(&format!(
            "noip_seconds_since_success{{host=\"{label}\"}} {}\n",
            now.saturating_sub(last)
        ));
        out.push_str(&format!(
            "noip_failures{{host=\"{label}\"}} {}\n",
            st.failures
        ));
        let blocked = st.blocked_until.map(|u| u.saturating_sub(now)).unwrap_or(0);
        out.push_str(&format!(
            "noip_blocked_seconds{{host=\"{label}\"}} {blocked}\n"
        ));
    }
    atomic_write(path, out.as_bytes(), 0o644)
}

pub(crate) fn sanitize_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
