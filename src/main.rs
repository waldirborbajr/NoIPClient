use std::{
    env,
    fs,
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use clap::Parser;
use serde::{Deserialize, Serialize};

const NOIP_UPDATE_URL: &str = "https://dynupdate.no-ip.com/nic/update";
const TIMEOUT: Duration = Duration::from_secs(10);
const APP_NAME: &str = "No-IP Client";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Fallback public-IP providers, tried in randomized order per run.
const IP_PROVIDERS: &[&str] = &[
    "https://api.ipify.org",
    "https://icanhazip.com",
    "https://ifconfig.me/ip",
    "https://ipinfo.io/ip",
];

/// No-IP `abuse` backoff: 30 minutes.
const ABUSE_BACKOFF: Duration = Duration::from_secs(30 * 60);

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "noip-client", version = APP_VERSION)]
struct Cli {
    /// Path to TOML config file.
    #[arg(short, long, env = "NOIP_CONFIG", default_value = "noip.toml")]
    config: PathBuf,

    /// Don't contact No-IP or Telegram; just report what would happen.
    #[arg(long)]
    dry_run: bool,

    /// Ignore local state and update every host now.
    #[arg(long)]
    force: bool,

    /// Ignore `silent_on_unchanged` and always notify.
    #[arg(long)]
    noisy: bool,

    /// Print the current state for each host and exit.
    #[arg(long)]
    status: bool,

    /// Loop internally instead of relying on cron.
    #[arg(long)]
    daemon: bool,

    /// Increase log verbosity (repeatable).
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_interval")]
    check_interval_secs: u64,
    #[serde(default = "default_ip_check")]
    ip_check_url: String,
    #[serde(default = "default_state_file")]
    state_file: PathBuf,
    prometheus_file: Option<PathBuf>,
    /// Accepts humantime-ish strings like "30s", "10m", "5h", "2d".
    #[serde(default = "default_force_after")]
    force_update_after: String,
    #[serde(default)]
    silent_on_unchanged: bool,

    noip: NoipConfig,
    #[serde(default)]
    telegram: Option<TelegramConfig>,
    hosts: Vec<HostConfig>,
}

#[derive(Debug, Deserialize)]
struct NoipConfig {
    user_agent: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    credentials_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
struct TelegramConfig {
    token: Option<String>,
    #[serde(default)]
    chat_ids: Vec<String>,
    #[serde(default)]
    silent_on_unchanged: bool,
}

impl TelegramConfig {
    fn enabled(&self) -> bool {
        self.token.as_deref().map_or(false, |t| !t.is_empty()) && !self.chat_ids.is_empty()
    }
}

#[derive(Debug, Deserialize, Clone)]
struct HostConfig {
    hostname: String,
    #[serde(default = "default_ip_version")]
    ip_version: u8,
    #[serde(default)]
    ip_check_url: Option<String>,
    #[serde(default)]
    silent_on_unchanged: Option<bool>,
}

fn default_interval() -> u64 { 300 }
fn default_ip_check() -> String { IP_PROVIDERS[0].into() }
fn default_state_file() -> PathBuf { "noip-state.json".into() }
fn default_force_after() -> String { "25d".into() }
fn default_ip_version() -> u8 { 4 }

impl Config {
    fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config {}", path.display()))?;
        if cfg.hosts.is_empty() {
            return Err(anyhow!("config has no [[hosts]] entries"));
        }
        for h in &cfg.hosts {
            if h.ip_version != 4 && h.ip_version != 6 {
                return Err(anyhow!(
                    "host {}: ip_version must be 4 or 6",
                    h.hostname
                ));
            }
        }
        Ok(cfg)
    }

    fn credentials(&self) -> Result<(String, String)> {
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

    fn force_update_after(&self) -> Result<u64> {
        parse_duration(&self.force_update_after)
            .ok_or_else(|| anyhow!("bad force_update_after: {:?}", self.force_update_after))
    }
}

/// Parse "30s", "10m", "5h", "2d" (bare integers mean seconds).
fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() { return None; }
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

// ---------------------------------------------------------------------------
// State (JSON, per host)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    #[serde(default)]
    hosts: std::collections::BTreeMap<String, HostState>,
}

impl StateFile {
    fn load(path: &Path) -> Self {
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

    fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        atomic_write(path, data.as_bytes(), 0o600)
    }

    fn host(&mut self, hostname: &str) -> &mut HostState {
        self.hosts.entry(hostname.to_string()).or_default()
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
struct HostState {
    /// Last IP we successfully registered.
    ip: Option<IpAddr>,
    /// Unix timestamp of last successful No-IP contact (good/nochg).
    last_success: Option<u64>,
    /// Consecutive failed runs.
    failures: u32,
    /// Unix timestamp until which we should not call No-IP (abuse/backoff).
    blocked_until: Option<u64>,
}

fn atomic_write(path: &Path, data: &[u8], _mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
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

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(TIMEOUT).build()
}

/// Try each provider in a shuffled order until one yields a valid public IP.
fn get_public_ip(explicit_url: Option<&str>, want_v6: bool) -> Result<IpAddr> {
    let mut providers: Vec<&str> = match explicit_url {
        Some(u) => vec![u],
        None => {
            let mut p: Vec<&str> = IP_PROVIDERS.to_vec();
            shuffle(&mut p);
            p
        }
    };
    if providers.is_empty() {
        providers.push(IP_PROVIDERS[0]);
    }

    let mut last_err: Option<anyhow::Error> = None;
    for url in providers {
        match fetch_ip(url, want_v6) {
            Ok(ip) => return Ok(ip),
            Err(e) => {
                log_debug(&format!("IP provider {url} failed: {e}"));
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("no IP providers configured")))
}

fn fetch_ip(url: &str, want_v6: bool) -> Result<IpAddr> {
    let resp = agent().get(url).call()?;
    let mut body = String::new();
    resp.into_reader().take(128).read_to_string(&mut body)?;
    let ip: IpAddr = body
        .trim()
        .parse()
        .with_context(|| format!("parsing {:?} from {url}", body.trim()))?;

    match ip {
        IpAddr::V4(v4) if !want_v6 => {
            if v4.is_private() || v4.is_loopback() || v4.is_link_local()
                || v4.is_unspecified() || v4.is_broadcast() || v4.is_multicast()
            {
                return Err(anyhow!("non-public IPv4 {v4}"));
            }
            Ok(ip)
        }
        IpAddr::V6(v6) if want_v6 => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return Err(anyhow!("non-public IPv6 {v6}"));
            }
            // Reject link-local fe80::/10 and unique-local fc00::/7.
            let seg = v6.segments()[0];
            if (seg & 0xffc0) == 0xfe80 || (seg & 0xfe00) == 0xfc00 {
                return Err(anyhow!("non-public IPv6 {v6}"));
            }
            Ok(ip)
        }
        _ => Err(anyhow!(
            "provider returned wrong family (wanted v{}, got {ip})",
            if want_v6 { 6 } else { 4 }
        )),
    }
}

/// Fisher–Yates using time as a cheap source of randomness (no `rand` dep).
fn shuffle<T>(v: &mut [T]) {
    let mut seed = now_secs() ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for i in (1..v.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (seed as usize) % (i + 1);
        v.swap(i, j);
    }
}

fn update_noip(
    user_agent: &str,
    username: &str,
    password: &str,
    hostname: &str,
    ip: IpAddr,
) -> Result<String> {
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    let request = agent()
        .get(NOIP_UPDATE_URL)
        .query("hostname", hostname)
        .query("myip", &ip.to_string())
        .set("Authorization", &format!("Basic {auth}"))
        .set("User-Agent", user_agent);

    match request.call() {
        Ok(resp) => Ok(resp.into_string()?),
        Err(ureq::Error::Status(_, resp)) => Ok(resp.into_string()?),
        Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>).map_err(Into::into),
    }
}

// ---------------------------------------------------------------------------
// Outcome classification
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Outcome {
    Updated,
    Unchanged,
    Retry(String),
    /// Terminal — do not retry.
    Fatal(String),
    /// Rate-limited — set `blocked_until`.
    Abuse,
}

fn classify(body: &str) -> Outcome {
    let code = body.split_whitespace().next().unwrap_or("");
    match code {
        "good" => Outcome::Updated,
        "nochg" => Outcome::Unchanged,
        "badauth" | "badagent" | "nohost" | "notfqdn" => Outcome::Fatal(code.into()),
        "abuse" => Outcome::Abuse,
        "911" => Outcome::Retry(code.into()),
        other => Outcome::Retry(format!("unexpected response: {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Per-host run
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunResult {
    Changed,
    Unchanged,
    Forced,
    Failure,
    Blocked,
    DryRun,
}

struct RunContext<'a> {
    cfg: &'a Config,
    username: &'a str,
    password: &'a str,
    force: bool,
    dry_run: bool,
    force_after: u64,
}

fn run_host(ctx: &RunContext<'_>, host: &HostConfig, st: &mut HostState) -> Result<RunResult> {
    // Abuse backoff gate.
    if let Some(until) = st.blocked_until {
        if now_secs() < until {
            log_warn(&format!(
                "{}: blocked for {} more seconds",
                host.hostname,
                until - now_secs()
            ));
            return Ok(RunResult::Blocked);
        } else {
            st.blocked_until = None;
        }
    }

    let want_v6 = host.ip_version == 6;
    let current = get_public_ip(
        host.ip_check_url
            .as_deref()
            .or(Some(ctx.cfg.ip_check_url.as_str())),
        want_v6,
    )?;

    let needs_force = match st.last_success {
        Some(t) => now_secs().saturating_sub(t) >= ctx.force_after,
        None => true,
    };
    let ip_changed = st.ip != Some(current);

    if !ctx.force && !needs_force && !ip_changed {
        st.failures = 0;
        return Ok(RunResult::Unchanged);
    }

    if ctx.dry_run {
        log_info(&format!(
            "{}: dry-run — would update to {current} (forced={}, changed={})",
            host.hostname, needs_force, ip_changed
        ));
        return Ok(RunResult::DryRun);
    }

    // Bounded retries for transient failures only.
    let mut attempt = 0u32;
    let mut delay = Duration::from_secs(2);
    let body = loop {
        attempt += 1;
        match update_noip(
            &ctx.cfg.noip.user_agent,
            ctx.username,
            ctx.password,
            &host.hostname,
            current,
        ) {
            Ok(body) => match classify(&body) {
                Outcome::Retry(reason) if attempt < 3 => {
                    log_warn(&format!(
                        "{}: retryable response ({reason}), attempt {attempt}/3",
                        host.hostname
                    ));
                    std::thread::sleep(delay);
                    delay *= 2;
                    continue;
                }
                _ => break body,
            },
            Err(e) if attempt < 3 => {
                log_warn(&format!(
                    "{}: network error ({e}), attempt {attempt}/3",
                    host.hostname
                ));
                std::thread::sleep(delay);
                delay *= 2;
                continue;
            }
            Err(e) => return Err(e),
        }
    };

    match classify(&body) {
        Outcome::Updated => {
            st.ip = Some(current);
            st.last_success = Some(now_secs());
            st.failures = 0;
            Ok(if ip_changed { RunResult::Changed } else { RunResult::Forced })
        }
        Outcome::Unchanged => {
            // No-IP says nothing to do; trust it over our local view.
            st.ip = Some(current);
            st.last_success = Some(now_secs());
            st.failures = 0;
            Ok(RunResult::Unchanged)
        }
        Outcome::Abuse => {
            st.blocked_until = Some(now_secs() + ABUSE_BACKOFF.as_secs());
            Err(anyhow!("No-IP says 'abuse' — backing off 30 minutes"))
        }
        Outcome::Fatal(code) => Err(anyhow!("No-IP rejected the update: {code}")),
        Outcome::Retry(reason) => Err(anyhow!("No-IP temporary error after retries: {reason}")),
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

fn host_message(
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
        lines.push(format!("Last successful update: {} ago", human_duration(now_secs().saturating_sub(t))));
    } else {
        lines.push("Last successful update: never".into());
    }
    if previous_failures > 0 && result != RunResult::Failure {
        lines.push(format!("Recovered after {} failed run(s)", previous_failures));
    }
    lines.push(format!("Time: {}", utc_now()));
    lines.push(format!("Machine: {}", machine_name()));
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Telegram
// ---------------------------------------------------------------------------

fn notify_telegram(cfg: &Option<TelegramConfig>, text: &str, silent: bool) -> Result<()> {
    let Some(tg) = cfg else { return Ok(()); };
    if !tg.enabled() {
        return Ok(());
    }
    let token = tg.token.as_deref().unwrap();
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");

    let mut last_err: Option<anyhow::Error> = None;
    for chat in &tg.chat_ids {
        let body = ureq::json!({
            "chat_id": chat,
            "text": text,
            "disable_notification": silent,
        });
        let res = agent()
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map(|_| ())
            .map_err(|e| anyhow!(e.to_string().replace(token, "***")));
        if let Err(e) = res {
            last_err = Some(e);
        }
    }
    match last_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Prometheus textfile
// ---------------------------------------------------------------------------

fn write_prometheus(path: Option<&Path>, state: &StateFile) -> Result<()> {
    let Some(path) = path else { return Ok(()); };
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

fn sanitize_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Main / daemon loop
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let exit = match run(cli) {
        Ok(code) => code,
        Err(e) => {
            log_error(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    };
    exit
}

fn run(cli: Cli) -> Result<ExitCode> {
    let cfg = Config::load(&cli.config)?;
    let (username, password) = cfg.credentials()?;
    let force_after = cfg.force_update_after()?;

    if cli.status {
        let state = StateFile::load(&cfg.state_file);
        for (host, st) in &state.hosts {
            let ip = st.ip.map(|i| i.to_string()).unwrap_or_else(|| "—".into());
            let last = st
                .last_success
                .map(|t| format!("{} ago", human_duration(now_secs().saturating_sub(t))))
                .unwrap_or_else(|| "never".into());
            let blocked = st
                .blocked_until
                .filter(|u| *u > now_secs())
                .map(|u| format!("{} remaining", human_duration(u - now_secs())))
                .unwrap_or_else(|| "—".into());
            println!(
                "Host: {host}\n  IP: {ip}\n  Last success: {last}\n  Failures: {}\n  Blocked: {blocked}",
                st.failures
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    let interval = Duration::from_secs(cfg.check_interval_secs.max(30));
    let mut run_ok = true;

    loop {
        run_ok &= run_once(&cli, &cfg, &username, &password, force_after);
        if !cli.daemon {
            break;
        }
        // Sleep in small slices so Ctrl-C / SIGTERM feels responsive.
        let deadline = Instant::now() + interval;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    if run_ok { Ok(ExitCode::SUCCESS) } else { Ok(ExitCode::FAILURE) }
}

fn run_once(
    cli: &Cli,
    cfg: &Config,
    username: &str,
    password: &str,
    force_after: u64,
) -> bool {
    let started = Instant::now();
    let mut state = StateFile::load(&cfg.state_file);
    let mut all_ok = true;

    for host in &cfg.hosts {
        let previous_failures = state.host(&host.hostname).failures;
        let host_state = state.host(&host.hostname).clone();
        let ctx = RunContext {
            cfg,
            username,
            password,
            force: cli.force,
            dry_run: cli.dry_run,
            force_after,
        };

        let result = {
            let hs = state.host(&host.hostname);
            run_host(&ctx, host, hs)
        };

        let (run_result, error) = match result {
            Ok(r) => (r, None),
            Err(e) => {
                let hs = state.host(&host.hostname);
                hs.failures = hs.failures.saturating_add(1);
                all_ok = false;
                (RunResult::Failure, Some(e.to_string()))
            }
        };

        let current_state = state.host(&host.hostname).clone();
        let text = host_message(
            host,
            &current_state,
            run_result,
            error.as_deref(),
            started.elapsed(),
            previous_failures,
        );

        // Log locally.
        if run_result == RunResult::Failure {
            log_error(&format!("{}\n{text}", host.hostname));
        } else {
            log_info(&text);
        }

        // Telegram: silent when unchanged unless --noisy.
        if !cli.dry_run {
            let silent = !cli.noisy
                && matches!(run_result, RunResult::Unchanged)
                && (cfg.silent_on_unchanged
                    || cfg.telegram.as_ref().map_or(true, |t| t.silent_on_unchanged));
            if let Err(e) = notify_telegram(&cfg.telegram, &text, silent) {
                log_warn(&format!("Telegram notification failed: {e}"));
            }
        }

        // Debug: show what changed (helps catch state bugs).
        log_debug(&format!(
            "{}: prev={:?} now={:?} result={:?}",
            host.hostname, host_state, current_state, run_result
        ));
    }

    if let Err(e) = state.save(&cfg.state_file) {
        log_warn(&format!("could not save state: {e}"));
    }
    if let Err(e) = write_prometheus(cfg.prometheus_file.as_deref(), &state) {
        log_warn(&format!("could not write Prometheus file: {e}"));
    }
    all_ok
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn human_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m {}s", secs / 60, secs % 60),
        3_600..=86_399 => format!("{}h {}m", secs / 3_600, (secs % 3_600) / 60),
        _ => format!("{}d {}h", secs / 86_400, (secs % 86_400) / 3_600),
    }
}

fn machine_name() -> String {
    fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn utc_now() -> String {
    let secs = now_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

// ---------------------------------------------------------------------------
// Logging (level-aware, tiny)
// ---------------------------------------------------------------------------

static VERBOSITY: AtomicBool = AtomicBool::new(false);
static DEBUG: AtomicBool = AtomicBool::new(false);

fn set_verbosity(level: u8) {
    VERBOSITY.store(level >= 1, Ordering::Relaxed);
    DEBUG.store(level >= 2, Ordering::Relaxed);
}

fn log_info(msg: &str) { println!("{msg}"); }
fn log_warn(msg: &str) { eprintln!("[warn] {msg}"); }
fn log_error(msg: &str) { eprintln!("[error] {msg}"); }
fn log_debug(msg: &str) {
    if DEBUG.load(Ordering::Relaxed) {
        eprintln!("[debug] {msg}");
    }
}
