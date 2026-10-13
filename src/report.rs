use std::time::Duration;

use crate::{
    config::HostConfig,
    consts::{APP_NAME, APP_VERSION},
    host::RunResult,
    state::HostState,
    util::{human_duration, machine_name, now_secs, utc_now},
};

pub(crate) fn host_message(
    host: &HostConfig,
    st: &HostState,
    result: RunResult,
    error: Option<&str>,
    elapsed: Duration,
    previous_failures: u32,
) -> String {
    let mut lines = vec![];
    let head = match result {
        RunResult::Changed => "🔄 IP CHANGED",
        RunResult::Forced => "♻️ Forced refresh",
        RunResult::Unchanged => "✅ Client running, no change",
        RunResult::Failure => "❌ No-IP client failure",
        RunResult::Blocked => "⛔ Backing off (abuse)",
        RunResult::DryRun => "🧪 Dry run",
    };
    lines.push(head.to_string());
    lines.push(format!("Program: {APP_NAME} v{APP_VERSION}"));
    lines.push(format!("DNS: {}", host.hostname));
    if let Some(ip) = st.ip {
        lines.push(format!("Current IP: {ip}"));
    }
    if let Some(err) = error {
        lines.push(format!("Error: {err}"));
        lines.push(format!("Consecutive failures: {}", previous_failures + 1));
    }
    lines.push(format!("Duration: {:.2}s", elapsed.as_secs_f64()));
    if let Some(t) = st.last_success {
        lines.push(format!(
            "Last successful update: {} ago",
            human_duration(now_secs().saturating_sub(t))
        ));
    } else {
        lines.push("Last successful update: never".into());
    }
    if previous_failures > 0 && result != RunResult::Failure {
        lines.push(format!(
            "Recovered after {} failed run(s)",
            previous_failures
        ));
    }
    lines.push(format!("Time: {}", utc_now()));
    lines.push(format!("Machine: {}", machine_name()));
    lines.join("\n")
}
