use base64::Engine;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

fn main() -> ExitCode {
    // Load .env if present. Under systemd, variables come from EnvironmentFile.
    let _ = dotenvy::dotenv();

    let cfg = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("[error] {e}");
            return ExitCode::FAILURE;
        }
    };

    let (text, code) = match run_once(&cfg) {
        Ok(status) => {
            let text = success_message(&cfg, &status);
            println!("{text}");
            (text, ExitCode::SUCCESS)
        }
        Err(e) => {
            let msg = e.to_string();
            eprintln!("[error] {msg}");
            (failure_message(&cfg, &msg), ExitCode::FAILURE)
        }
    };

    // Telegram is sent on every run, success or failure.
    if let Err(e) = notify_telegram(&cfg, &text) {
        eprintln!("[warn] Telegram notification failed: {e}");
    }

    code
}

fn run_once(cfg: &Config) -> Result<Status, Box<dyn Error>> {
    let current = get_public_ip(&cfg.ip_check_url)?;
    let previous = read_last_ip(&cfg.state_file);

    if previous == Some(current) {
        return Ok(Status::Unchanged { current });
    }

    let body = update_noip(cfg, current)?;

    match classify(&body) {
        Outcome::Updated | Outcome::Unchanged => {
            write_last_ip(&cfg.state_file, current)?;
            Ok(Status::Changed { previous, current })
        }
        Outcome::Fatal(code) => Err(format!("No-IP rejected the update: {code}").into()),
        Outcome::Retry(reason) => Err(format!("No-IP temporary error: {reason}").into()),
    }
}

fn success_message(cfg: &Config, status: &Status) -> String {
    let now = utc_now();
    let machine = machine_name();

    match status {
        Status::Changed { previous, current } => {
            let previous = previous
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "none (first run)".into());
            format!(
                "🔄 IP CHANGED\n\
                 Program: {} v{}\n\
                 DNS: {}\n\
                 New IP: {}\n\
                 Previous IP: {}\n\
                 Time: {}\n\
                 Machine: {}",
                APP_NAME, APP_VERSION, cfg.hostname, current, previous, now, machine
            )
        }
        Status::Unchanged { current } => format!(
            "✅ Client running, no change\n\
             Program: {} v{}\n\
             DNS: {}\n\
             Current IP: {}\n\
             Time: {}\n\
             Machine: {}",
            APP_NAME, APP_VERSION, cfg.hostname, current, now, machine
        ),
    }
}

fn failure_message(cfg: &Config, error: &str) -> String {
    format!(
        "❌ No-IP client failure\n\
         Program: {} v{}\n\
         DNS: {}\n\
         Error: {}\n\
         Time: {}\n\
         Machine: {}",
        APP_NAME,
        APP_VERSION,
        cfg.hostname,
        error,
        utc_now(),
        machine_name()
    )
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

fn read_last_ip(path: &PathBuf) -> Option<Ipv4Addr> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn write_last_ip(path: &PathBuf, ip: Ipv4Addr) -> Result<(), Box<dyn Error>> {
    fs::write(path, ip.to_string())?;
    Ok(())
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

/// Machine name from /etc/hostname, for telling several servers apart.
fn machine_name() -> String {
    fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

/// Current UTC time as "YYYY-MM-DD HH:MM:SS UTC", without extra crates.
fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

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
