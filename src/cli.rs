use std::path::PathBuf;

use clap::Parser;

use crate::consts::APP_VERSION;

#[derive(Parser, Debug)]
#[command(name = "noipclient", version = APP_VERSION)]
pub(crate) struct Cli {
    /// Path to TOML config file.
    #[arg(short, long, env = "NOIP_CONFIG", default_value = "noip.toml")]
    pub(crate) config: PathBuf,

    /// Interactively create a config and credentials file.
    #[arg(
        long,
        conflicts_with_all = ["dry_run", "force", "noisy", "status", "daemon"]
    )]
    pub(crate) init: bool,

    /// Don't contact No-IP or Telegram; just report what would happen.
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Ignore local state and update every host now.
    #[arg(long)]
    pub(crate) force: bool,

    /// Ignore `silent_on_unchanged` and always notify.
    #[arg(long)]
    pub(crate) noisy: bool,

    /// Print the current state for each host and exit.
    #[arg(long)]
    pub(crate) status: bool,

    /// Loop internally instead of relying on cron.
    #[arg(long)]
    pub(crate) daemon: bool,

    /// Increase log verbosity; -vv enables debug output.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,
}
