use base64::Engine;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const NOIP_UPDATE_URL: &str = "https://dynupdate.no-ip.com/nic/update";
const TIMEOUT: Duration = Duration::from_secs(10);

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

    match run_once(&cfg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[error] {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_once(cfg: &Config) -> Result<(), Box<dyn Error>> {
    let ip = get_public_ip(&cfg.ip_check_url)?;

    if read_last_ip(&cfg.state_file) == Some(ip) {
        return Ok(()); // no change since last run
    }

    let body = update_noip(cfg, ip)?;

    match classify(&body) {
        Outcome::Updated => {
            println!("[info] {} -> {ip}", cfg.hostname);
            write_last_ip(&cfg.state_file, ip)?;
            if let Err(e) = notify_telegram(cfg, ip) {
                eprintln!("[warn] Telegram notification failed: {e}");
            }
            Ok(())
        }
        Outcome::Unchanged => {
            println!("[info] {} already points to {ip}", cfg.hostname);
            write_last_ip(&cfg.state_file, ip)?;
            Ok(())
        }
        Outcome::Fatal(code) => Err(format!("No-IP rejected the update: {code}").into()),
        Outcome::Retry(reason) => Err(format!("No-IP temporary error: {reason}").into()),
    }
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

fn notify_telegram(cfg: &Config, ip: Ipv4Addr) -> Result<(), Box<dyn Error>> {
    let (Some(token), Some(chat_id)) = (&cfg.telegram_token, &cfg.telegram_chat_id) else {
        return Ok(()); // notifications not configured
    };

    let text = format!("No-IP update\nDNS name: {}\nNew IP: {}", cfg.hostname, ip);
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");

    agent()
        .post(&url)
        .send_form(&[("chat_id", chat_id.as_str()), ("text", text.as_str())])?;

    Ok(())
}
