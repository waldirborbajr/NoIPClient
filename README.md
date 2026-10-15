# No-IP Client

<p align="center">
  <img width="256" height="256" src="./assets/logo.png" />
</p>

[![Rust CI](https://github.com/waldirborbajr/NoIPClient/actions/workflows/rust-ci.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/rust-ci.yaml)
[![Release](https://github.com/waldirborbajr/NoIPClient/actions/workflows/release.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/release.yaml)
[![Release to crates.io](https://github.com/waldirborbajr/NoIPClient/actions/workflows/crates.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/crates.yaml)
[![Security Audit](https://github.com/waldirborbajr/NoIPClient/actions/workflows/security-audit.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/security-audit.yaml)

A Rust client that keeps one or more No-IP hostnames (https://www.noip.com/) pointed at your current public IP.

It runs in one of three ways: as a systemd timer (default), as a long-lived daemon (`--daemon`), or in the background on Termux or a notebook with the included `noipctl.sh` script. It is safe against transient network failures, honors No-IP's abuse rate limit, and can notify you on Telegram on every run, or only when something changed.

## Features

- **Multiple hosts** - each `[[hosts]]` entry is tracked independently, with its own IP family, provider override, and Telegram silent policy.
- **IPv4 and IPv6** - pick `ip_version = 4` or `6` per host. IPv6 uses AAAA records on No-IP.
- **Multiple IP providers** - api.ipify.org, icanhazip.com, ifconfig.me/ip, ipinfo.io/ip. Tried in randomized order per run; the first successful response wins.
- **Validates the detected IP** - rejects private, loopback, link-local, broadcast, multicast, unspecified, IPv6 ULA (fc00::/7), and IPv6 link-local (fe80::/10) addresses.
- **Only updates No-IP when needed** - the last successful IP is kept in a JSON state file.
- **Forced refresh** - No-IP expires hostnames that are not updated for about 30 days. The client forces an update every `force_update_after` (default `25d`).
- **Retry with exponential backoff** - transient errors and No-IP `911` responses are retried up to 3 times per run.
- **Abuse backoff** - if No-IP returns `abuse`, the host is skipped for 30 minutes.
- **Atomic state writes** - temp file plus rename, `0600` permissions on Unix.
- **Telegram notifications** - optional, multi-recipient, with a status message on every run. Silent on unchanged runs if configured.
- **Device details on Termux** - status messages include model, Android version, memory, storage, battery, and network addresses when running on Android/Termux.
- **Prometheus textfile output** - optional.
- **Small binary** - release profile with LTO and stripped symbols.

## Requirements

- Rust 1.90 or newer (the crate uses edition 2024, and some dependencies need 1.88+)
- A No-IP account with one or more active hostnames
- (Optional) A Telegram bot
- (Optional) Docker, for building Android binaries with the included `Dockerfile`

## Build

Build the release binary for this machine:

```bash
cargo build --release
```

The binary is written to `target/release/noipclient`.

The project includes a `justfile` with the common tasks. Install `just` (`cargo install just`, or your package manager), then run `just` to list them:

| Recipe | What it does |
|---|---|
| `just build` | Native release build |
| `just check` | `cargo fmt --check`, clippy and tests |
| `just termux` | Build one Termux binary with `cross` and package it (arm64 by default) |
| `just termux-all` | Build all three Termux targets with `cross` |
| `just docker-termux` | Build one Termux binary with the Dockerfile and package it |
| `just docker-termux-all` | Build all three Termux targets with the Dockerfile |
| `just docker-termux-clean` | Build with Docker, then remove all Docker data |
| `just docker-clean` | Remove all Docker containers, images, volumes, networks, and build cache |
| `just clean` | Remove `target/` and `dist/` |

Packages are written to `dist/`. Each Termux package is named `dist/noipclient-<version>-<target>.tar.gz` and contains the binary and `noip.toml.example`.

### Termux binaries with Docker

The Dockerfile cross-compiles for Android without a host Android toolchain. Docker must be running on the build machine (on NixOS: `virtualisation.docker.enable = true;` and add your user to the `docker` group).

```bash
just docker-termux                         # arm64, most phones
just docker-termux armv7-linux-androideabi # older 32-bit phones
```

Use `just docker-termux-clean` to remove all Docker data after the build. This also deletes the base image, so the next build downloads it again. It affects every Docker project on the machine.

Docker runs on the build machine only. Termux does not run Docker; the phone just runs the binary.

## Configuration

A minimal config:

```toml
[noip]
user_agent = "noipclient/0.2 you@example.com"
username   = "you@example.com"
password   = "your-password"

[[hosts]]
hostname   = "yourhost.ddns.net"
ip_version = 4
```

A more complete config:

```toml
# Global defaults; per-host overrides are possible.
check_interval_secs = 300
ip_check_url        = "https://api.ipify.org"
state_file          = "/var/lib/noipclient/state.json"
prometheus_file     = "/var/lib/node_exporter/textfile/noip.prom"
force_update_after  = "25d"
silent_on_unchanged = true

[noip]
user_agent       = "noipclient/0.2 you@example.com"
credentials_file = "/etc/noipclient/credentials"

[telegram]
token               = "123456789:ABCdefGHIjklMNOpqrsTUVwxyz"
chat_ids            = ["123456789"]
silent_on_unchanged = true

[[hosts]]
hostname   = "home.ddns.net"
ip_version = 4

[[hosts]]
hostname   = "vpn.ddns.net"
ip_version = 4

[[hosts]]
hostname   = "home-v6.ddns.net"
ip_version = 6
```

A ready-to-edit copy is in `noip.toml.example`.

### Config reference

Top level:

| Key | Default | Notes |
|---|---|---|
| `check_interval_secs` | `300` | Only used with `--daemon`. Minimum 30. |
| `ip_check_url` | `https://api.ipify.org` | Default for hosts that do not override it. |
| `state_file` | `noip-state.json` | JSON; created if missing. Use an absolute path under a service or cron. |
| `prometheus_file` | unset | If set, metrics are written here. |
| `force_update_after` | `25d` | `s`, `m`, `h`, and `d` suffixes are accepted. |
| `silent_on_unchanged` | `false` | Telegram `disable_notification` on runs without a change. |

`[noip]`:

| Key | Required | Notes |
|---|---|---|
| `user_agent` | yes | No-IP requires a real contact address. |
| `username` / `password` | one of | Inline credentials. |
| `credentials_file` | one of | First two non-empty lines: username, then password. |

`[telegram]` (all optional):

| Key | Notes |
|---|---|
| `token` | Bot token from @BotFather. |
| `chat_ids` | Array of chat IDs. Group chat IDs are negative. |
| `silent_on_unchanged` | Skip the notification sound on runs without a change. |

`[[hosts]]` (one or more required):

| Key | Default | Notes |
|---|---|---|
| `hostname` | - | The No-IP hostname to update. |
| `ip_version` | `4` | `4` or `6`. |
| `ip_check_url` | inherited | Override the IP provider for this host. |
| `silent_on_unchanged` | inherited | Override the Telegram silent policy for this host. |

Paths in the config are not expanded: `~` and environment variables do not work. Use absolute paths.

### Credentials file

```bash
sudo install -d -m 0750 /etc/noipclient
sudo tee /etc/noipclient/credentials >/dev/null <<'EOT'
you@example.com
your-password
EOT
sudo chmod 600 /etc/noipclient/credentials
```

### Telegram setup

1. Message @BotFather on Telegram, send `/newbot`, and copy the token.
2. Send any message to your new bot.
3. Open `https://api.telegram.org/bot<TOKEN>/getUpdates` in a browser and find `"chat":{"id":...}`. That number is the chat ID. Group chat IDs are negative.

### Telegram status messages

Each run sends a status message. The first line says what happened. Example from a notebook (x86_64):

```text
✅ Client running, no change
Program: No-IP Client v0.2.0
DNS: home.ddns.net
Current IP: 203.0.113.7
Duration: 0.41s
Last successful update: 3h 12m ago
Time: 2026-10-13 14:05:00 UTC
Machine: homeserver-01
```

When the IP changes, the first line is `🔄 IP CHANGED`, followed by the new IP. The message does not include the previous address.

On Termux, the same message also includes a device section:

```text
✅ Client running, no change
Program: No-IP Client v0.2.0
DNS: home.ddns.net
Current IP: 192.168.1.13
Duration: 0.63s
Last successful update: 1h 4m ago
Time: 2026-10-13 14:05:00 UTC
Machine: unknown
--- Device ---
📱 Device: Xiaomi Mi 9 Lite
🤖 Android: 11 (API 30)
📦 Termux: 0.118.3
🖥️ Architecture: aarch64
🧠 CPU cores: 6
🔄 Uptime: 2d 14h
👤 User: u0_a253
🌐 Local IP: 192.168.1.13
🌐 wlan0 IP: 192.168.1.13
💾 Memory total: 5.49 GB
💾 Memory available: 3.24 GB
📀 Storage total: 108.89 GB
📀 Storage free: 99.21 GB
🔋 Battery: 100% (FULL)
```

`Machine` shows `unknown` on Termux because `/etc/hostname` does not exist there. Some values can be `unknown` or `unavailable` depending on the device and Android version.

## Command line

```text
Usage: noipclient [OPTIONS]

Options:
  -c, --config <CONFIG>   Path to TOML config [env: NOIP_CONFIG]
      --dry-run           Don't contact No-IP or Telegram
      --force             Update every host now, ignoring local state
      --noisy             Ignore silent_on_unchanged
      --status            Print state for each host and exit
      --daemon            Loop internally instead of relying on a timer
  -v, --verbose...        Increase verbosity; -vv enables debug output
  -h, --help              Print help
  -V, --version           Print version
```

### Examples

```bash
noipclient --config /etc/noipclient/noip.toml
noipclient --force --noisy
noipclient --dry-run -vv
noipclient --status --config /etc/noipclient/noip.toml
noipclient --daemon --config /etc/noipclient/noip.toml
```

## Manual test

Create `noip.toml` (see Configuration). Start with a dry run, which contacts neither No-IP nor Telegram:

```bash
./target/release/noipclient --config noip.toml --dry-run -vv
```

Then run it for real:

```bash
./target/release/noipclient --config noip.toml
```

## Running in the background with `noipctl.sh`

`noipctl.sh` starts the daemon in the background with `&` and stores its PID, so `stop` only ends the process it started. It works on Termux and on Linux (including notebooks). It uses `/bin/sh` and needs no extra tools.

Commands:

```text
noipctl.sh start      start the daemon in the background
noipctl.sh stop       stop it (SIGTERM, then SIGKILL after 10 seconds)
noipctl.sh restart    stop and start
noipctl.sh status     show whether it is running and its PID
noipctl.sh log        follow the log (Ctrl+C to exit)
```

Default paths are in `$HOME/noip`:

| Item | Default | Variable |
|---|---|---|
| Directory | `$HOME/noip` | `NOIP_DIR` |
| Binary | `$NOIP_DIR/noipclient` | `NOIP_BIN` |
| Config | `$NOIP_DIR/noip.toml` | `NOIP_CONFIG` |
| Log | `$NOIP_DIR/noip.log` | `NOIP_LOG` |
| PID file | `$NOIP_DIR/noipclient.pid` | `NOIP_PIDFILE` |

### On Termux

```bash
mkdir -p ~/noip
tar xzf noipclient-*-aarch64-linux-android.tar.gz -C ~/noip
mv ~/noip/noip.toml.example ~/noip/noip.toml     # then edit it
chmod 600 ~/noip/noip.toml
cp noipctl.sh ~/noip/ && chmod +x ~/noip/noipctl.sh

~/noip/noipctl.sh start
~/noip/noipctl.sh status
```

Use absolute paths in `noip.toml`, for example `state_file = "/data/data/com.termux/files/home/noip/state.json"`.

On Termux, the process does not survive closing the session if it was started with `&`, and it does not survive a phone reboot. Run `termux-wake-lock` to keep Android from suspending it. For automatic start after reboot, use Termux:Boot to run `noipctl.sh start`.

### On a notebook

Build the binary, then run the script with the paths pointing to your files:

```bash
cargo build --release
NOIP_BIN=./target/release/noipclient \
NOIP_CONFIG=./noip.toml \
NOIP_DIR=/tmp/noip \
./noipctl.sh start
```

If you want it to start automatically at login or after a reboot, use the systemd setup below instead of the script.

## Running with systemd

Use the systemd setup on NixOS or any Linux distribution that uses systemd. The examples below use a dedicated `noip` user, so the configuration file containing your password or Telegram token is readable only by that user and root.

### NixOS

Add this to `configuration.nix` (or to an imported module):

```nix
{ config, pkgs, lib, ... }:

let
  noipclient = pkgs.rustPlatform.buildRustPackage {
    pname = "noipclient";
    version = "0.2.0";
    src = /path/to/NoIPClient;              # or a flake input
    cargoLock.lockFile = /path/to/NoIPClient/Cargo.lock;
  };
in
{
  users.users.noip = {
    isSystemUser = true;
    group = "noip";
  };
  users.groups.noip = { };

  # Config directory: root-owned, readable by the noip group.
  systemd.tmpfiles.rules = [
    "d /etc/noipclient 0750 root noip -"
  ];

  systemd.services.noipclient = {
    description = "No-IP dynamic DNS update";
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];

    serviceConfig = {
      Type = "oneshot";
      User = "noip";
      Group = "noip";
      ExecStart = "${noipclient}/bin/noipclient --config /etc/noipclient/noip.toml";
      StateDirectory = "noipclient";        # /var/lib/noipclient, writable by the service
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      NoNewPrivileges = true;
    };
  };

  systemd.timers.noipclient = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnBootSec = "1min";
      OnUnitActiveSec = "5min";
      Persistent = true;
    };
  };
}
```

Create the configuration file outside the Nix store (it contains secrets):

```bash
sudo install -o noip -g noip -m 0600 noip.toml.example /etc/noipclient/noip.toml
sudo $EDITOR /etc/noipclient/noip.toml
```

Set the state file to the service directory:

```toml
state_file = "/var/lib/noipclient/state.json"
```

Rebuild and test:

```bash
sudo nixos-rebuild switch
sudo systemctl start noipclient.service
journalctl -u noipclient.service -n 30 --no-pager
systemctl list-timers noipclient.timer
sudo -u noip noipclient --status --config /etc/noipclient/noip.toml
```

If you enable `prometheus_file`, the directory must be writable by the service. Add `ReadWritePaths = [ "/var/lib/node_exporter/textfile" ];` to `serviceConfig`.

### Generic Linux (systemd)

Install the binary and the configuration:

```bash
sudo install -m 0755 target/release/noipclient /usr/local/bin/noipclient
sudo useradd --system --home-dir /var/lib/noipclient --shell /usr/sbin/nologin noip
sudo install -d -m 0750 -o root -g noip /etc/noipclient
sudo install -o noip -g noip -m 0600 noip.toml.example /etc/noipclient/noip.toml
sudo $EDITOR /etc/noipclient/noip.toml
```

Service unit, `/etc/systemd/system/noipclient.service`:

```ini
[Unit]
Description=No-IP dynamic update check
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User=noip
Group=noip
ExecStart=/usr/local/bin/noipclient --config /etc/noipclient/noip.toml
StateDirectory=noipclient
WorkingDirectory=/var/lib/noipclient
```

Timer unit, `/etc/systemd/system/noipclient.timer`:

```ini
[Unit]
Description=Run noipclient periodically

[Timer]
OnBootSec=1min
OnUnitActiveSec=5min
Persistent=true

[Install]
WantedBy=timers.target
```

Enable the timer:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now noipclient.timer
```

### Long-lived daemon (systemd)

Instead of the timer, you can run the client as a daemon that checks on its own interval (`check_interval_secs`):

```ini
[Unit]
Description=No-IP dynamic update client (daemon)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=noip
Group=noip
ExecStart=/usr/local/bin/noipclient --daemon --config /etc/noipclient/noip.toml
Restart=on-failure
StateDirectory=noipclient
WorkingDirectory=/var/lib/noipclient

[Install]
WantedBy=multi-user.target
```

Use either the timer or the daemon, not both.

## Prometheus metrics

When `prometheus_file` is set, the client writes a textfile with one set of values per host:

```text
noip_last_success_timestamp{host="home.ddns.net"} 1730000000
noip_seconds_since_success{host="home.ddns.net"} 42
noip_failures{host="home.ddns.net"} 0
noip_blocked_seconds{host="home.ddns.net"} 0
```

## Verification

```bash
systemctl list-timers noipclient.timer
journalctl -u noipclient.service -n 50
journalctl -u noipclient.service -f
sudo systemctl start noipclient.service
noipclient --status --config /etc/noipclient/noip.toml
```

## Troubleshooting

- **no No-IP credentials** - set `noip.username` and `noip.password`, or `noip.credentials_file` in `noip.toml`.
- **reading credentials ...** - the file is missing, or the service user cannot read it.
- **No-IP rejected the update: badauth** - wrong username or password.
- **No-IP rejected the update: badagent** - `noip.user_agent` does not meet No-IP's policy.
- **non-public IPv4 / IPv6** - the provider returned a private or reserved address. If your ISP uses CGNAT, your address is in `100.64.0.0/10`, which this check does not reject.
- **provider returned wrong family** - check `ip_version` and `ip_check_url`.
- **Telegram notification failed** - check `token` and `chat_ids`, and make sure you sent a message to the bot first.
- **state file corrupt** - the client discards the file and starts fresh.
- **Hostname does not update even though No-IP returns `good`** - free No-IP hostnames require periodic confirmation. Check your No-IP dashboard.
- **Paths not found on Termux or cron** - `~` and environment variables are not expanded. Use absolute paths.

## Security

- `noip.toml` should be `chmod 600` (or `640` with a group the service uses). The credentials file should be `chmod 600`.
- Do not put the configuration file in the Nix store or in version control.
- The client redacts the Telegram token from error messages. Avoid sharing logs that may contain other sensitive values.
- Use a `noip.user_agent` with a real contact address that you monitor.
- On Termux, if you use key-based SSH, disable password login in `sshd_config`.

## Migration from the env-var version

| Old env var | New TOML key |
|---|---|
| `NOIP_HOSTNAME` | `[[hosts]].hostname` |
| `NOIP_USERNAME` | `noip.username` |
| `NOIP_PASSWORD` | `noip.password` |
| `NOIP_USER_AGENT` | `noip.user_agent` |
| `IP_CHECK_URL` | `ip_check_url` |
| `STATE_FILE` | `state_file` |
| `TELEGRAM_BOT_TOKEN` | `telegram.token` |
| `TELEGRAM_CHAT_ID` | `telegram.chat_ids` |
| `CHECK_INTERVAL_SECS` | `check_interval_secs` |

## Project layout

| File | Purpose |
|---|---|
| `src/main.rs` | Entry point, run loop, and per-run orchestration |
| `src/cli.rs` | Command-line arguments |
| `src/config.rs` | TOML configuration and credentials |
| `src/consts.rs` | Endpoints, timeouts, and provider list |
| `src/state.rs` | Per-host state file (JSON) and atomic writes |
| `src/host.rs` | Update logic for one host |
| `src/ip.rs` | Public IP lookup and validation |
| `src/noip.rs` | No-IP update request and response handling |
| `src/report.rs` | Status message text |
| `src/telegram.rs` | Telegram delivery |
| `src/prometheus.rs` | Prometheus textfile output |
| `src/platform.rs` | Termux and Android detection |
| `src/termux.rs` | Device details shown on Termux |
| `src/util.rs` | Time, durations, and helpers |
| `src/log.rs` | Console logging and `-v` / `-vv` |
| `noipctl.sh` | Start, stop, and status script for background runs |
| `Dockerfile` | Cross-compiles the Android binaries |
| `justfile` | Build, check, package, and Docker recipes |

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

Choose the license you prefer.
