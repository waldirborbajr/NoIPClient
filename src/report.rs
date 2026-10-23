use std::time::Duration;

use crate::{
    config::HostConfig,
    consts::{APP_NAME, APP_VERSION},
    host::RunResult,
    platform,
    state::HostState,
    termux,
    util::{html_escape, human_duration, machine_name, now_secs, utc_now},
};

const RULE: &str = "━━━━━━━━━━━━━━━━━━━━";

pub(crate) fn host_message(
    host: &HostConfig,
    st: &HostState,
    result: RunResult,
    error: Option<&str>,
    elapsed: Duration,
    previous_failures: u32,
) -> String {
    let head = match result {
        RunResult::Changed => "🔄 IP CHANGED",
        RunResult::Forced => "♻️ Forced refresh",
        RunResult::Unchanged => "✅ Client running, no change",
        RunResult::Failure => "❌ No-IP client failure",
        RunResult::Blocked => "⛔ Backing off (abuse)",
        RunResult::DryRun => "🧪 Dry run",
    };

    let last_update = match st.last_success {
        Some(t) => format!("{} ago", human_duration(now_secs().saturating_sub(t))),
        None => "never".into(),
    };

    let mut lines: Vec<String> = vec![format!("<b>{head}</b>"), RULE.into()];
    lines.push("🌐 <b>No-IP</b>".into());
    lines.push(field("Program", &format!("{APP_NAME} v{APP_VERSION}")));
    lines.push(field("DNS", &host.hostname));
    if let Some(ip) = st.ip {
        lines.push(field("IP", &ip.to_string()));
    }
    if let Some(err) = error {
        lines.push(field("Error", err));
        lines.push(field(
            "Failures in a row",
            &(previous_failures + 1).to_string(),
        ));
    }
    lines.push(field("Run time", &format!("{:.2}s", elapsed.as_secs_f64())));
    lines.push(field("Last update", &last_update));
    if previous_failures > 0 && result != RunResult::Failure {
        lines.push(field(
            "Recovered after",
            &format!("{previous_failures} failed run(s)"),
        ));
    }
    lines.push(field("Time", &utc_now()));
    let machine = if platform::is_termux() {
        termux::machine_name()
    } else {
        machine_name()
    };
    lines.push(field("Machine", &machine));

    if platform::is_termux() {
        lines.push(RULE.into());
        lines.extend(termux::device_lines());
    }
    lines.join("\n")
}

fn field(label: &str, value: &str) -> String {
    format!("• {label}: <code>{}</code>", html_escape(value))
}
