//! Decides whether a run produces a notification, and sends it through the chosen channel.

use anyhow::Result;

use crate::{
    config::{Channel, Config, Frequency},
    email,
    host::RunResult,
    telegram::notify_telegram,
};

/// True when this run should notify, given the channel and the chosen frequency.
pub(crate) fn should_notify(channel: Channel, frequency: Frequency, result: RunResult) -> bool {
    if channel == Channel::Nothing {
        return false;
    }
    match frequency {
        Frequency::Always => true,
        Frequency::Change => result == RunResult::Changed,
        Frequency::Failure => result == RunResult::Failure,
    }
}

/// Subject for email, e.g. "[NoIPClient] home.ddns.net: IP changed".
pub(crate) fn subject(hostname: &str, result: RunResult) -> String {
    let what = match result {
        RunResult::Changed => "IP changed",
        RunResult::Failure => "update failed",
        RunResult::Unchanged => "no change",
        RunResult::Forced => "forced refresh",
        RunResult::Blocked => "backing off",
        RunResult::DryRun => "dry run",
    };
    format!("[NoIPClient] {hostname}: {what}")
}

/// Sends the message through the configured channel. `silent` only affects Telegram.
pub(crate) fn send(cfg: &Config, subject: &str, text: &str, silent: bool) -> Result<()> {
    let (channel, _) = cfg.notify_settings();
    match channel {
        Channel::Telegram => notify_telegram(&cfg.telegram, text, silent),
        Channel::Email => match &cfg.email {
            Some(email_cfg) => email::send(email_cfg, subject, &strip_html(text)),
            None => Ok(()),
        },
        Channel::Nothing => Ok(()),
    }
}

/// Status messages use HTML tags for Telegram; email receives plain text.
pub(crate) fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_never_notifies() {
        for f in [Frequency::Always, Frequency::Change, Frequency::Failure] {
            assert!(!should_notify(Channel::Nothing, f, RunResult::Failure));
        }
    }

    #[test]
    fn always_notifies_every_result() {
        for r in [RunResult::Changed, RunResult::Unchanged, RunResult::Failure] {
            assert!(should_notify(Channel::Telegram, Frequency::Always, r));
        }
    }

    #[test]
    fn change_notifies_only_on_ip_change() {
        assert!(should_notify(
            Channel::Email,
            Frequency::Change,
            RunResult::Changed
        ));
        assert!(!should_notify(
            Channel::Email,
            Frequency::Change,
            RunResult::Unchanged
        ));
        assert!(!should_notify(
            Channel::Email,
            Frequency::Change,
            RunResult::Failure
        ));
    }

    #[test]
    fn failure_notifies_only_on_failure() {
        assert!(should_notify(
            Channel::Telegram,
            Frequency::Failure,
            RunResult::Failure
        ));
        assert!(!should_notify(
            Channel::Telegram,
            Frequency::Failure,
            RunResult::Changed
        ));
        assert!(!should_notify(
            Channel::Telegram,
            Frequency::Failure,
            RunResult::Unchanged
        ));
    }

    #[test]
    fn strips_tags_and_unescapes_entities() {
        assert_eq!(
            strip_html("<b>DNS</b>: <code>a&amp;b &lt;x&gt;</code>"),
            "DNS: a&b <x>"
        );
    }
}
