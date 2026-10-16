//! Device information shown in Telegram status messages when running on Termux.
//! Uses only the standard library plus commands that exist in Termux.

use std::{env, fs, net::UdpSocket, process::Command, thread};

use crate::util::{html_escape, human_duration};

/// Device section for a status message, as Telegram HTML lines.
pub(crate) fn device_lines() -> Vec<String> {
    let manufacturer = getprop("ro.product.manufacturer");
    let model = getprop("ro.product.model");
    let device = match (manufacturer, model) {
        (Some(m), Some(d)) => format!("{m} {d}"),
        (None, Some(d)) => d,
        _ => "unknown".into(),
    };
    let android = format!(
        "{} (API {})",
        or_unknown(getprop("ro.build.version.release")),
        or_unknown(getprop("ro.build.version.sdk")),
    );
    let (mem_total, mem_avail) = (meminfo_kb("MemTotal:"), meminfo_kb("MemAvailable:"));
    let (disk_total, disk_free) = storage().unwrap_or((None, None));
    let (battery_pct, battery_status) = battery();
    let battery_text = match battery_pct {
        Some(p) => format!("{p}% ({battery_status})"),
        None => "unavailable".into(),
    };

    vec![
        "📱 <b>Device</b>".into(),
        row("Model", &device),
        row("Android", &android),
        row("Termux", &or_unknown(env::var("TERMUX_VERSION").ok())),
        row("Architecture", env::consts::ARCH),
        row("CPU cores", &cpu_cores().to_string()),
        row("Uptime", &uptime().unwrap_or_else(|| "unknown".into())),
        row("User", &current_user()),
        String::new(),
        "🌐 <b>Network</b>".into(),
        row(
            "Local IP",
            &local_ip().unwrap_or_else(|| "unavailable".into()),
        ),
        row(
            "wlan0 IP",
            &iface_ipv4("wlan0").unwrap_or_else(|| "unavailable".into()),
        ),
        String::new(),
        "💾 <b>Memory</b>".into(),
        row(
            "Total",
            &mem_total.map(fmt_kb).unwrap_or_else(|| "unknown".into()),
        ),
        row(
            "Available",
            &mem_avail.map(fmt_kb).unwrap_or_else(|| "unknown".into()),
        ),
        String::new(),
        "📀 <b>Storage</b>".into(),
        row(
            "Total",
            &disk_total
                .map(fmt_bytes)
                .unwrap_or_else(|| "unknown".into()),
        ),
        row(
            "Free",
            &disk_free.map(fmt_bytes).unwrap_or_else(|| "unknown".into()),
        ),
        String::new(),
        "🔋 <b>Battery</b>".into(),
        row("Charge", &battery_text),
    ]
}

/// One "label: value" row, with the value in monospace and escaped for HTML.
fn row(label: &str, value: &str) -> String {
    format!("• {label}: <code>{}</code>", html_escape(value))
}

fn or_unknown(v: Option<String>) -> String {
    v.unwrap_or_else(|| "unknown".into())
}

/// Run a command and return its trimmed stdout.
/// A non-zero exit status is ignored when there is output, because tools such as
/// `ifconfig` print a warning and exit non-zero on Android. Falls back to $PREFIX/bin
/// when the command is not on PATH.
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .or_else(|_| {
            let prefix =
                env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".into());
            Command::new(format!("{prefix}/bin/{cmd}"))
                .args(args)
                .output()
        })
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn getprop(key: &str) -> Option<String> {
    run("getprop", &[key])
}

fn current_user() -> String {
    run("whoami", &[])
        .or_else(|| env::var("USER").ok())
        .unwrap_or_else(|| "unknown".into())
}

fn cpu_cores() -> usize {
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0)
}

fn uptime() -> Option<String> {
    // /proc/uptime is often unreadable from Termux on Android, so fall back to `uptime`.
    let secs = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|t| t.split_whitespace().next()?.parse::<f64>().ok())
        .map(|v| v as u64)
        .or_else(|| run("uptime", &[]).and_then(|out| parse_uptime_output(&out)))?;
    Some(human_duration(secs))
}

/// Parse the output of `uptime`, e.g. " 23:07:15 up 2 days, 14:22,  0 users,  load average: ...".
/// Returns the uptime in seconds.
fn parse_uptime_output(text: &str) -> Option<u64> {
    let after = text.split(" up ").nth(1)?;
    let (mut days, mut hours, mut mins) = (0u64, 0u64, 0u64);
    for part in after.split(',') {
        let part = part.trim();
        if part.contains("user") || part.contains("load average") {
            continue;
        }
        if part.contains("day") {
            days = part.split_whitespace().next()?.parse().ok()?;
        } else if part.contains(':') {
            let mut it = part.split(':');
            hours = it.next()?.trim().parse().ok()?;
            mins = it.next()?.trim().parse().ok()?;
        } else if part.contains("min") {
            mins = part.split_whitespace().next()?.parse().ok()?;
        } else if part.contains("hour") {
            hours = part.split_whitespace().next()?.parse().ok()?;
        }
    }
    Some(days * 86_400 + hours * 3_600 + mins * 60)
}

/// Value from /proc/meminfo in kB, e.g. key = "MemTotal:".
fn meminfo_kb(key: &str) -> Option<u64> {
    fs::read_to_string("/proc/meminfo")
        .ok()?
        .lines()
        .find(|l| l.starts_with(key))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn fmt_kb(kb: u64) -> String {
    fmt_bytes(kb * 1024)
}

fn fmt_bytes(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

/// (total, free) bytes for the Termux home filesystem, via `df`.
fn storage() -> Option<(Option<u64>, Option<u64>)> {
    let home = env::var("HOME").unwrap_or_else(|_| "/".into());
    let out = run("df", &["-P", "-k", &home])?;
    let fields: Vec<&str> = out.lines().nth(1)?.split_whitespace().collect();
    let total = fields.get(1)?.parse::<u64>().ok()? * 1024;
    let free = fields.get(3)?.parse::<u64>().ok()? * 1024;
    Some((Some(total), Some(free)))
}

/// Battery from sysfs, falling back to Termux:API when sysfs is not readable.
fn battery() -> (Option<u32>, String) {
    let sysfs_cap = fs::read_to_string("/sys/class/power_supply/battery/capacity")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if let Some(cap) = sysfs_cap {
        let status = fs::read_to_string("/sys/class/power_supply/battery/status")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".into());
        return (Some(cap), status);
    }

    match run("termux-battery-status", &[]) {
        Some(json) => {
            let pct = json_field(&json, "percentage").and_then(|v| v.parse::<u32>().ok());
            let status = json_field(&json, "status").unwrap_or_else(|| "unknown".into());
            (pct, status)
        }
        None => (None, "unknown".into()),
    }
}

/// Minimal extraction of a `"key": value` pair from simple JSON text.
fn json_field(text: &str, key: &str) -> Option<String> {
    let start = text.find(&format!("\"{key}\""))?;
    let rest = &text[start + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let end = rest.find([',', '}', '\n']).unwrap_or(rest.len());
    Some(rest[..end].trim().trim_matches('"').to_string())
}

/// Primary outgoing IPv4 address. Connecting a UDP socket sends no packets.
fn local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// IPv4 address of a named interface, via `ip`. May fail on newer Android versions.
fn iface_ipv4(name: &str) -> Option<String> {
    ip_command_ipv4(name).or_else(|| ifconfig_ipv4(name))
}

fn ip_command_ipv4(name: &str) -> Option<String> {
    let out = run("ip", &["-4", "-o", "addr", "show", name])?;
    out.split_whitespace()
        .skip_while(|t| *t != "inet")
        .nth(1)
        .map(|cidr| cidr.split('/').next().unwrap_or(cidr).to_string())
}

/// `ifconfig` works on Termux where `ip` is blocked. Handles both
/// "inet 192.168.1.13" and the older "inet addr:192.168.1.13" formats.
fn ifconfig_ipv4(name: &str) -> Option<String> {
    let out = run("ifconfig", &[name])?;
    out.split_whitespace()
        .skip_while(|t| *t != "inet")
        .nth(1)
        .map(|addr| addr.trim_start_matches("addr:").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_numbers_and_strings_from_single_line_json() {
        let json = r#"{"percentage": 87, "status": "CHARGING", "plugged": "AC"}"#;
        assert_eq!(json_field(json, "percentage").as_deref(), Some("87"));
        assert_eq!(json_field(json, "status").as_deref(), Some("CHARGING"));
    }

    #[test]
    fn reads_fields_from_pretty_printed_json() {
        let json = "{\n  \"percentage\": 42,\n  \"status\": \"DISCHARGING\"\n}";
        assert_eq!(json_field(json, "percentage").as_deref(), Some("42"));
        assert_eq!(json_field(json, "status").as_deref(), Some("DISCHARGING"));
    }

    #[test]
    fn missing_key_returns_none() {
        assert_eq!(json_field("{\"status\": \"FULL\"}", "percentage"), None);
    }
}

#[cfg(test)]
mod uptime_tests {
    use super::*;

    #[test]
    fn parses_uptime_with_days_and_hh_mm() {
        let out = " 23:07:15 up 2 days, 14:22,  0 users,  load average: 0.00, 0.00, 0.00";
        assert_eq!(
            parse_uptime_output(out),
            Some(2 * 86_400 + 14 * 3_600 + 22 * 60)
        );
    }

    #[test]
    fn parses_uptime_under_a_day_with_minutes() {
        let out = " 10:00:00 up 45 min,  1 user,  load average: 0.10, 0.20, 0.30";
        assert_eq!(parse_uptime_output(out), Some(45 * 60));
    }

    #[test]
    fn returns_none_without_up_field() {
        assert_eq!(parse_uptime_output("garbage"), None);
    }
}
