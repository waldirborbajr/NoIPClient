# noip-client

A Rust client that updates a No-IP hostname with your current public IP.
Each run checks the IP, updates No-IP only if the IP has changed, and can
optionally send a Telegram message. It is designed to run from a systemd timer.

## Features

- Detects the public IP through an external service (default: `api.ipify.org`)
- Validates the detected IP before sending it (rejects private and invalid addresses)
- Updates No-IP only when the IP changes
- Stores the last sent IP in a state file
- Optional Telegram notification with the DNS name and new IP

## Requirements

- Rust (stable toolchain, via `rustup` or `nix-shell -p cargo rustc`)
- A No-IP account with an active hostname
- (Optional) A Telegram bot

## Build

```bash
cargo build --release
```

The binary is written to `target/release/noip-client`.

## Configuration

Create a `.env` file in the directory where the program will run:

```env
NOIP_HOSTNAME=yourhost.ddns.net
NOIP_USERNAME=your_login
NOIP_PASSWORD=your_password
NOIP_USER_AGENT="noip-rust-client/0.1 you@example.com"

# Optional
IP_CHECK_URL=https://api.ipify.org
STATE_FILE=/var/lib/noip-client/last_ip.txt
TELEGRAM_BOT_TOKEN=123456789:ABCdefGHIjklMNOpqrsTUVwxyz
TELEGRAM_CHAT_ID=123456789
```

Notes:

- Quote the `NOIP_USER_AGENT` value, since it contains a space.
- The `.env` file contains your password. Restrict access with `chmod 600 .env`.
- Do not commit `.env` to version control. Add it to `.gitignore`.

### Telegram setup (optional)

1. Message `@BotFather` on Telegram, send `/newbot`, and copy the token.
2. Send any message to your new bot.
3. Open `https://api.telegram.org/bot<TOKEN>/getUpdates` in a browser and find
   `"chat":{"id":...}`. That number is your `TELEGRAM_CHAT_ID`.

If both Telegram variables are unset, the program runs without notifications.

## Manual test

Before setting up the timer, run the program once from the directory containing `.env`:

```bash
./target/release/noip-client
```

Expected output:

- `[info] yourhost.ddns.net -> <your IP>` on the first run, or when the IP has changed
- No output, or only errors, on runs where the IP has not changed

Check that the state file was created:

```bash
cat /var/lib/noip-client/last_ip.txt
```

## systemd timer setup

### NixOS

Add this to your `configuration.nix` (or to an imported module):

```nix
let
  noipClient = pkgs.rustPlatform.buildRustPackage {
    pname = "noip-client";
    version = "0.1.0";
    src = /path/to/noip-client;
    cargoLock.lockFile = /path/to/noip-client/Cargo.lock;
  };
in
{
  systemd.services.noip-client = {
    description = "No-IP dynamic update check";
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${noipClient}/bin/noip-client";
      WorkingDirectory = "/var/lib/noip-client";
      StateDirectory = "noip-client";
      EnvironmentFile = "/var/lib/noip-client/.env";
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

After editing, copy the `.env` file and rebuild:

```bash
sudo mkdir -p /var/lib/noip-client
sudo cp .env /var/lib/noip-client/.env
sudo chmod 600 /var/lib/noip-client/.env
sudo nixos-rebuild switch
```

Notes:

- `EnvironmentFile` loads the `.env` through systemd. `dotenvy` also reads the file, so either path works.
- `DynamicUser = true` runs the service as a temporary user. If that user cannot read `.env`, adjust the permissions, or remove this line and use a dedicated user.
- To skip building with Nix, replace `ExecStart` with the path to a binary you compiled yourself and copied to `/var/lib/noip-client/`.

### Generic Linux (systemd)

1. Copy the binary and the `.env` file:

```bash
sudo mkdir -p /var/lib/noip-client
sudo cp target/release/noip-client /var/lib/noip-client/
sudo cp .env /var/lib/noip-client/.env
sudo chmod 600 /var/lib/noip-client/.env
```

2. Create `/etc/systemd/system/noip-client.service`:

```ini
[Unit]
Description=No-IP dynamic update check
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
WorkingDirectory=/var/lib/noip-client
ExecStart=/var/lib/noip-client/noip-client
StateDirectory=noip-client
EnvironmentFile=/var/lib/noip-client/.env
```

3. Create `/etc/systemd/system/noip-client.timer`:

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

4. Enable the timer:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now noip-client.timer
```

## Verification

```bash
# Show the next scheduled runs
systemctl list-timers noip-client.timer

# Logs from recent runs
journalctl -u noip-client.service -n 50

# Follow logs in real time
journalctl -u noip-client.service -f

# Run an immediate check
sudo systemctl start noip-client.service
```

## Troubleshooting

- **`missing required env var NOIP_USER_AGENT`**: the `.env` file is not being read.
  Check the working directory and that the variable name is spelled exactly.
- **`No-IP rejected the update: badauth`**: wrong username or password.
  Fix the `.env` file. The timer keeps retrying on each run until this is resolved.
- **`No-IP rejected the update: badagent`**: the `NOIP_USER_AGENT` does not meet No-IP's policy.
  Check No-IP's current documentation.
- **`lookup service returned a non-public address`**: the IP lookup service returned a non-public address.
  If your ISP uses CGNAT, your address is in `100.64.0.0/10`, which this check does not reject.
- **Hostname does not update even though No-IP returns `good`**: free No-IP hostnames require periodic
  confirmation. Check your No-IP dashboard.

## Security

- The `.env` file contains your password and Telegram token. Keep it at `chmod 600`.
- Do not log the Telegram token. Network errors can include the request URL.
- Use a `NOIP_USER_AGENT` with a real contact address that you monitor.

## License

Choose the license you prefer.
