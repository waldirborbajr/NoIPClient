# No-IP Client

<p align="center">
  <img width="256" height="256" src="./assets/logo.png" />
</p>

[![Rust CI](https://github.com/waldirborbajr/NoIPClient/actions/workflows/rust-ci.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/rust-ci.yaml)
[![Release](https://github.com/waldirborbajr/NoIPClient/actions/workflows/release.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/release.yaml)
[![Release to crates.io](https://github.com/waldirborbajr/NoIPClient/actions/workflows/crates.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/crates.yaml)
[![Security Audit](https://github.com/waldirborbajr/NoIPClient/actions/workflows/security-audit.yaml/badge.svg)](https://github.com/waldirborbajr/NoIPClient/actions/workflows/security-audit.yaml)

A Rust client that keeps one or more No-IP hostnames (https://www.noip.com/) pointed at your current public IP.

Designed to run either as a systemd timer/oneshot (default) or as a long-lived daemon (--daemon). It is safe against transient network failures, honors No-IP's abuse rate limit, and can notify you on Telegram only when something actually changed.

## Features

- **Multiple hosts** - each [[hosts]] entry is tracked independently, with its own IP family, provider override, and Telegram silent policy.
- **IPv4 and IPv6** - pick ip_version = 4 or 6 per host. IPv6 uses AAAA records on No-IP.
- **Multiple IP providers** - api.ipify.org, icanhazip.com, ifconfig.me/ip, ipinfo.io/ip. Tried in randomized order per run; the first successful response wins.
- **Validates the detected IP** - rejects private, loopback, link-local, broadcast, multicast, unspecified, IPv6 ULA (fc00::/7), and IPv6 link-local (fe80::/10).
- **Only updates No-IP when needed** - the last successful IP is kept in a JSON state file.
- **Forced refresh** - No-IP expires hostnames that are not touched for ~30 days. Forced refresh every force_update_after (default 25d).
- **Retry with exponential backoff** - transient errors and No-IP 911 responses are retried up to 3 times per run.
- **abuse backoff** - if No-IP returns abuse, the host is skipped for 30 minutes.
- **Atomic state writes** - temp file + rename, 0600 permissions on Unix.
- **Telegram notifications** - optional, multi-recipient, silent for unchanged runs.
- **Prometheus textfile output** - optional.
- **Tiny binary** - release profile with LTO and stripped symbols.

## Requirements

- Rust 1.85 or newer (the crate uses edition 2024)
- A No-IP account with one or more active hostnames
- (Optional) A Telegram bot

## Build

Build the release binary for this machine:

```bash
cargo build --release
```

The binary is written to `target/release/noip-client`.

The project includes a `justfile` with the common tasks. Install `just` (`cargo install just`, or your package manager), then run `just` to list them:

| Recipe | What it does |
|---|---|
| `just build` | Native release build |
| `just check` | `cargo fmt --check`, clippy and tests |
| `just termux` | Build one Termux binary (arm64 by default) and package it |
| `just termux aarch64-linux-android` | Same, for a specific target |
| `just termux-all` | Build all three Termux targets |
| `just clean` | Remove `target/` and `dist/` |

Packages are written to `dist/`.

## Configuration

A minimal config:

```toml
[noip]
user_agent = "noip-client/0.2 you@example.com"
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
state_file          = "/var/lib/noip-client/state.json"
prometheus_file     = "/var/lib/node_exporter/textfile/noip.prom"
force_update_after  = "25d"
silent_on_unchanged = true

[noip]
user_agent = "noip-client/0.2 you@example.com"
credentials_file = "/etc/noip-client/credentials"

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

### Config reference

Top level:

| Key | Default | Notes |
|---|---|---|
| check_interval_secs | 300 | Only used with --daemon. Minimum 30. |
| ip_check_url | https://api.ipify.org | Default for hosts that do not override it. |
| state_file | noip-state.json | JSON; created if missing. |
| prometheus_file | unset | If set, metrics are written here. |
| force_update_after | 25d | s/m/h/d suffixes accepted. |
| silent_on_unchanged | false | Telegram disable_notification on no-op runs. |

[noip]:

| Key | Required | Notes |
|---|---|---|
| user_agent | yes | No-IP requires a real contact address. |
| username / password | one of | Inline credentials. |
| credentials_file | one of | First two non-empty lines: username, password. |

[telegram] (all optional):

| Key | Notes |
|---|---|
| token | Bot token from @BotFather. |
| chat_ids | Array of chat IDs. Groups are negative. |
| silent_on_unchanged | Skip the notification sound on no-op runs. |

[[hosts]] (one or more required):

| Key | Default | Notes |
|---|---|---|
| hostname | - | The No-IP hostname to update. |
| ip_version | 4 | 4 or 6. |
| ip_check_url | inherited | Override the provider for this host. |
| silent_on_unchanged | inherited | Override the Telegram silent policy. |

### Credentials file

```bash
sudo install -d -m 0750 /etc/noip-client
sudo tee /etc/noip-client/credentials >/dev/null <<'EOF'
you@example.com
your-password
EOF
sudo chmod 600 /etc/noip-client/credentials
```

### Telegram setup

1. Message @BotFather, send /newbot, copy the token.
2. Send any message to your new bot.
3. Open https://api.telegram.org/bot<TOKEN>/getUpdates and find "chat":{"id":...}.

## Command line

```text
Usage: noip-client [OPTIONS]

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
noip-client --config /etc/noip-client/noip.toml
noip-client --force --noisy
noip-client --dry-run -vv
noip-client --status
noip-client --daemon
```

## Manual test

Create `noip.toml` (see Configuration). Start with a dry run, which contacts neither No-IP nor Telegram:

```bash
./target/release/noip-client --config noip.toml --dry-run -vv
```

Then run it for real:

```bash
./target/release/noip-client --config noip.toml
```

## Running with systemd

### NixOS

```nix
let
  noipClient = pkgs.rustPlatform.buildRustPackage {
    pname = "noip-client";
    version = "0.2.0";
    src = /path/to/NoIPClient;
    cargoLock.lockFile = /path/to/NoIPClient/Cargo.lock;
  };
in
{
  systemd.services.noip-client = {
    description = "No-IP dynamic update check";
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${noipClient}/bin/noip-client --config /var/lib/noip-client/noip.toml";
      WorkingDirectory = "/var/lib/noip-client";
      StateDirectory = "noip-client";
      DynamicUser = true;
    };
  };

  systemd.timers.noip-client = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnBootSec = "1min";
      OnUnitActiveSec = "5min";
      Persistent = true;
    };
  };
}
```

### Generic Linux (systemd)

Service unit:

```ini
[Unit]
Description=No-IP dynamic update check
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/noip-client --config /etc/noip-client/noip.toml
StateDirectory=noip-client
WorkingDirectory=/var/lib/noip-client
```

Timer unit:

```ini
[Unit]
Description=Run noip-client periodically

[Timer]
OnBootSec=1min
OnUnitActiveSec=5min
Persistent=true

[Install]
WantedBy=timers.target
```

Enable:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now noip-client.timer
```

### Long-lived daemon

```ini
[Unit]
Description=No-IP dynamic update client (daemon)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/noip-client --daemon --config /etc/noip-client/noip.toml
Restart=on-failure
StateDirectory=noip-client
WorkingDirectory=/var/lib/noip-client

[Install]
WantedBy=multi-user.target
```

## Termux (Android)

Termux has no systemd, so run the client from cron or start it by hand.

Get the binary, using the asset that matches your phone:

- `noip-client-vX.Y.Z-aarch64-linux-android.tar.gz`: 64-bit ARM phones (most devices)
- `noip-client-vX.Y.Z-armv7-linux-androideabi.tar.gz`: older 32-bit phones
- `noip-client-vX.Y.Z-x86_64-linux-android.tar.gz`: x86_64 emulators

Or build it yourself. The Termux builds use `cross`, which runs the Android toolchain in Docker, so Docker must be running:

```bash
just termux                              # arm64, the most common phone
just termux armv7-linux-androideabi      # older 32-bit phones
just termux-all                          # all three
```

Each run writes `dist/noip-client-<version>-<target>.tar.gz`, which contains the binary and `noip.toml.example`. The recipe checks that the binary architecture matches the target before packaging.

On NixOS, enable Docker with `virtualisation.docker.enable = true;` and add your user to the `docker` group.

Install it and set up the config. Use absolute paths in `noip.toml`: the client does not expand `~` or environment variables.

```bash
pkg install cronie
mkdir -p ~/noip
tar xzf noip-client-*-aarch64-linux-android.tar.gz -C ~/noip
mv ~/noip/noip.toml.example ~/noip/noip.toml   # then edit it
chmod 600 ~/noip/noip.toml
```

Schedule it every 5 minutes:

```bash
crontab -e
# add:
*/5 * * * * $HOME/noip/noip-client --config $HOME/noip/noip.toml >> $HOME/noip/noip.log 2>&1
```

Start the cron daemon with `crond`. Add it to `~/.termux/boot/` with Termux:Boot to start it after reboot, and `termux-wake-lock` to keep Android from suspending it.

## Prometheus metrics

```text
noip_last_success_timestamp{host="home.ddns.net"} 1730000000
noip_seconds_since_success{host="home.ddns.net"} 42
noip_failures{host="home.ddns.net"} 0
noip_blocked_seconds{host="home.ddns.net"} 0
```

## Verification

```bash
systemctl list-timers noip-client.timer
journalctl -u noip-client.service -n 50
journalctl -u noip-client.service -f
sudo systemctl start noip-client.service
noip-client --status --config /etc/noip-client/noip.toml
```

## Troubleshooting

- **no No-IP credentials** - set noip.username and noip.password, or noip.credentials_file in noip.toml.
- **reading credentials ...** - the file is missing or unreadable by the service user.
- **No-IP rejected the update: badauth** - wrong username or password.
- **No-IP rejected the update: badagent** - noip.user_agent does not meet No-IP's policy.
- **non-public IPv4 / IPv6** - the provider returned a private or reserved address.
- **provider returned wrong family** - check ip_version and ip_check_url.
- **Telegram notification failed** - check token and chat_ids.
- **state file corrupt** - the client discards the file and starts fresh.
- **paths not found on Termux or cron** - `~` and environment variables are not expanded in `noip.toml`. Use absolute paths.

## Security

- noip.toml should be chmod 640 or 600. The credentials file should be chmod 600.
- The client redacts the Telegram token from error messages.
- Use a noip.user_agent with a real contact address.

## Migration from the env-var version

| Old env var | New TOML key |
|---|---|
| NOIP_HOSTNAME | [[hosts]].hostname |
| NOIP_USERNAME | noip.username |
| NOIP_PASSWORD | noip.password |
| NOIP_USER_AGENT | noip.user_agent |
| IP_CHECK_URL | ip_check_url |
| STATE_FILE | state_file |
| TELEGRAM_BOT_TOKEN | telegram.token |
| TELEGRAM_CHAT_ID | telegram.chat_ids |
| CHECK_INTERVAL_SECS | check_interval_secs |

## License

Choose the license you prefer.
