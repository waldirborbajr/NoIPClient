use base64::Engine;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NOIP_UPDATE_URL: &str = "https://dynupdate.no-ip.com/nic/update";
const TIMEOUT: Duration = Duration::from_secs(10);
const APP_NAME: &str = "No-IP Client";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Config {
    hostname: String,
    username: String,
    password: String,
    user_agent: String,
    ip_check_url: String,
    state_file: PathBuf,
    telegram_token: Option<String>,
    telegram_chat_id: Option<String>,
}

enum Outcome {
    Updated,
    Unchanged,
    Fatal(String),
    Retry(String),
}

enum Status {
    Changed {
        previous: Option<Ipv4Addr>,
        current: Ipv4Addr,
    },
    Unchanged {
        current: Ipv4Addr,
    },
}

impl Status {
    fn current(&self) -> Ipv4Addr {
        match self {
            Status::Changed { current, .. } | Status::Unchanged { current } => *current,
        }
    }
}

/// State kept between runs, stored as three lines:
/// IP, unix timestamp of the last success, consecutive failures.
/// A legacy file containing only the IP is still accepted.
#[derive(Default)]
struct State {
    ip: Option<Ipv4Addr>,
    last_success: Option<u64>,
    failures: u32,
}

impl State {
    fn load(path: &Path) -> State {
        let Ok(text) = fs::read_to_string(path) else {
            return State::default();
        };
        let mut lines = text.lines().map(str::trim);
        State {
            ip: lines.next().and_then(|l| l.parse().ok()),
            last_success: lines.next().and_then(|l| l.parse().ok()),
            failures: lines.next().and_then(|l| l.parse().ok()).unwrap_or(0),
        }
    }

    fn save(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        let ip = self.ip.map(|ip| ip.to_string()).unwrap_or_default();
        let last = self.last_success.map(|t| t.to_string()).unwrap_or_default();
        fs::write(path, format!("{ip}\n{last}\n{}\n", self.failures))?;
        Ok(())
    }
}

/// Run metrics shared by the success and failure messages.
struct Report {
    elapsed: Duration,
    previous_failures: u32,
    since_last_success: Option<u64>,
}

impl Report {
    fn last_success_line(&self) -> String {
        match self.since_last_success {
            Some(secs) => format!("{} ago", human_duration(secs)),
            None => "never".into(),
        }
    }

    fn duration_line(&self) -> String {
        format!("{:.2}s", self.elapsed.as_secs_f64())
    }
}

fn main() -> ExitCode {
    // Load .env if present. Under systemd, variables come from EnvironmentFile.
    let _ = dotenvy::dotenv();
    let started = Instant::now();

    let cfg = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("[error] {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut state = State::load(&cfg.state_file);
    let previous_failures = state.failures;
    let since_last_success = state.last_success.map(|t| now_secs().saturating_sub(t));

    let (text, code) = match run_once(&cfg, &state) {
        Ok(status) => {
            let report = Report {
                elapsed: started.elapsed(),
                previous_failures,
                since_last_success,
            };
            let text = success_message(&cfg, &status, &report);
            println!("{text}");

            state.ip = Some(status.current());
            state.last_success = Some(now_secs());
            state.failures = 0;
            (text, ExitCode::SUCCESS)
        }
        Err(e) => {
            let msg = e.to_string();
            eprintln!("[error] {msg}");

            let report = Report {
                elapsed: started.elapsed(),
                previous_failures,
                since_last_success,
            };
            state.failures = previous_failures.saturating_add(1);
            (failure_message(&cfg, &msg, &report), ExitCode::FAILURE)
        }
    };

    if let Err(e) = state.save(&cfg.state_file) {
        eprintln!("[warn] could not save state: {e}");
    }

    // Telegram is sent on every run, success or failure.
    if let Err(e) = notify_telegram(&cfg, &text) {
        eprintln!("[warn] Telegram notification failed: {e}");
    }

    code
}

fn run_once(cfg: &Config, state: &State) -> Result<Status, Box<dyn Error>> {
    let current = get_public_ip(&cfg.ip_check_url)?;

    if state.ip == Some(current) {
        return Ok(Status::Unchanged { current });
    }

    let body = update_noip(cfg, current)?;

    match classify(&body) {
        Outcome::Updated | Outcome::Unchanged => Ok(Status::Changed {
            previous: state.ip,
            current,
        }),
        Outcome::Fatal(code) => Err(format!("No-IP rejected the update: {code}").into()),
        Outcome::Retry(reason) => Err(format!("No-IP temporary error: {reason}").into()),
    }
}

fn success_message(cfg: &Config, status: &Status, report: &Report) -> String {
    let now = utc_now();
    let machine = machine_name();

    let mut lines = match status {
        Status::Changed { previous, current } => {
            let previous = previous
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "none (first run)".into());
            vec![
                "🔄 IP CHANGED".to_string(),
                format!("Program: {APP_NAME} v{APP_VERSION}"),
                format!("DNS: {}", cfg.hostname),
                format!("New IP: {current}"),
                format!("Previous IP: {previous}"),
            ]
        }
        Status::Unchanged { current } => vec![
            "✅ Client running, no change".to_string(),
            format!("Program: {APP_NAME} v{APP_VERSION}"),
            format!("DNS: {}", cfg.hostname),
            format!("Current IP: {current}"),
        ],
    };

    lines.push(format!("Time: {now}"));
    lines.push(format!("Machine: {machine}"));
    lines.push(format!("Duration: {}", report.duration_line()));
    lines.push(format!(
        "Last successful update: {}",
        report.last_success_line()
    ));
    if report.previous_failures > 0 {
        lines.push(format!(
            "Recovered after {} failed run(s)",
            report.previous_failures
        ));
    }

    lines.join("\n")
}

fn failure_message(cfg: &Config, error: &str, report: &Report) -> String {
    [
        "❌ No-IP client failure".to_string(),
        format!("Program: {APP_NAME} v{APP_VERSION}"),
        format!("DNS: {}", cfg.hostname),
        format!("Error: {error}"),
        format!("Consecutive failures: {}", report.previous_failures + 1),
        format!("Last successful update: {}", report.last_success_line()),
        format!("Duration: {}", report.duration_line()),
        format!("Time: {}", utc_now()),
        format!("Machine: {}", machine_name()),
    ]
    .join("\n")
}

fn load_config() -> Result<Config, Box<dyn Error>> {
    Ok(Config {
        hostname: require("NOIP_HOSTNAME")?,
        username: require("NOIP_USERNAME")?,
        password: require("NOIP_PASSWORD")?,
        user_agent: require("NOIP_USER_AGENT")?,
        ip_check_url: env::var("IP_CHECK_URL").unwrap_or_else(|_| "https://api.ipify.org".into()),
        state_file: env::var("STATE_FILE")
            .unwrap_or_else(|_| "last_ip.txt".into())
            .into(),
        telegram_token: optional("TELEGRAM_BOT_TOKEN"),
        telegram_chat_id: optional("TELEGRAM_CHAT_ID"),
    })
}

fn require(key: &str) -> Result<String, Box<dyn Error>> {
    let value = env::var(key).map_err(|_| format!("missing required env var {key}"))?;
    if value.trim().is_empty() {
        return Err(format!("env var {key} is empty").into());
    }
    Ok(value)
}

fn optional(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.trim().is_empty())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(TIMEOUT).build()
}

fn get_public_ip(url: &str) -> Result<Ipv4Addr, Box<dyn Error>> {
    let resp = agent().get(url).call()?;

    // A public IP reply is under 16 bytes, so reject anything larger.
    let mut body = String::new();
    resp.into_reader().take(64).read_to_string(&mut body)?;

    let ip: Ipv4Addr = body.trim().parse()?;

    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
    {
        return Err(format!("lookup service returned a non-public address: {ip}").into());
    }

    Ok(ip)
}

fn update_noip(cfg: &Config, ip: Ipv4Addr) -> Result<String, Box<dyn Error>> {
    let credentials = format!("{}:{}", cfg.username, cfg.password);
    let auth = base64::engine::general_purpose::STANDARD.encode(credentials);

    let request = agent()
        .get(NOIP_UPDATE_URL)
        .query("hostname", &cfg.hostname)
        .query("myip", &ip.to_string())
        .set("Authorization", &format!("Basic {auth}"))
        .set("User-Agent", &cfg.user_agent);

    match request.call() {
        Ok(resp) => Ok(resp.into_string()?),
        // No-IP can return error codes with a non-2xx status; the body still holds the code.
        Err(ureq::Error::Status(_, resp)) => Ok(resp.into_string()?),
        Err(e) => Err(Box::new(e)),
    }
}

fn classify(body: &str) -> Outcome {
    let code = body.split_whitespace().next().unwrap_or("");

    match code {
        "good" => Outcome::Updated,
        "nochg" => Outcome::Unchanged,
        "badauth" | "badagent" | "nohost" | "abuse" | "notfqdn" => Outcome::Fatal(code.to_string()),
        "911" => Outcome::Retry(code.to_string()),
        other => Outcome::Retry(format!("unexpected response: {other:?}")),
    }
}

fn notify_telegram(cfg: &Config, text: &str) -> Result<(), String> {
    let (Some(token), Some(chat_id)) = (&cfg.telegram_token, &cfg.telegram_chat_id) else {
        return Ok(()); // notifications not configured
    };

    let url = format!("https://api.telegram.org/bot{token}/sendMessage");

    agent()
        .post(&url)
        .send_form(&[("chat_id", chat_id.as_str()), ("text", text)])
        // Redact the token in case the error message contains the URL.
        .map_err(|e| e.to_string().replace(token.as_str(), "***"))?;

    Ok(())
}

/// Seconds since the Unix epoch.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Compact duration, e.g. "45s", "12m 3s", "5h 20m", "2d 4h".
fn human_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m {}s", secs / 60, secs % 60),
        3_600..=86_399 => format!("{}h {}m", secs / 3_600, (secs % 3_600) / 60),
        _ => format!("{}d {}h", secs / 86_400, (secs % 86_400) / 3_600),
    }
}

/// Machine name from /etc/hostname, for telling several servers apart.
fn machine_name() -> String {
    fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

/// Current UTC time as "YYYY-MM-DD HH:MM:SS UTC", without extra crates.
fn utc_now() -> String {
    let secs = now_secs() as i64;

    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);

    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
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
