mod cli;
mod config;
mod consts;
mod email;
mod host;
mod http;
mod init;
mod ip;
mod log;
mod noip;
mod notify;
mod platform;
mod prometheus;
mod report;
mod state;
mod telegram;
mod termux;
mod util;

use std::{
    process::ExitCode,
    time::{Duration, Instant},
};

use anyhow::Result;
use clap::Parser;

use cli::Cli;
use config::Config;
use host::{run_host, RunContext, RunResult};
use log::{log_debug, log_error, log_info, log_warn, set_verbosity};
use prometheus::write_prometheus;
use report::host_message;
use state::StateFile;
use util::{human_duration, now_secs};

fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            log_error(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    set_verbosity(cli.verbose);
    if cli.init {
        init::initialize(&cli.config)?;
        return Ok(ExitCode::SUCCESS);
    }

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

    if run_ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

fn run_once(cli: &Cli, cfg: &Config, username: &str, password: &str, force_after: u64) -> bool {
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

        // Notifications: channel and frequency come from [notify].
        if !cli.dry_run {
            let (channel, frequency) = cfg.notify_settings();
            if notify::should_notify(channel, frequency, run_result) {
                // Per-host setting wins; otherwise global or Telegram-level setting.
                let policy = host.silent_on_unchanged.unwrap_or(
                    cfg.silent_on_unchanged
                        || cfg.telegram.as_ref().is_some_and(|t| t.silent_on_unchanged),
                );
                let silent = !cli.noisy && matches!(run_result, RunResult::Unchanged) && policy;
                let subject = notify::subject(&host.hostname, run_result);
                if let Err(e) = notify::send(cfg, &subject, &text, silent) {
                    log_warn(&format!("notification failed: {e}"));
                }
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
