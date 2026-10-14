# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/).

## [0.2.0] - Unreleased

### Added

- TOML configuration file (`noip.toml`), replacing the `.env` configuration.
  Select the file with `--config` or the `NOIP_CONFIG` environment variable.
- Multiple hosts per configuration, each with its own `ip_version` (IPv4 or IPv6).
- Multiple Telegram recipients through `chat_ids`.
- `silent_on_unchanged` setting, available globally, per host, and per Telegram configuration.
- Command-line options: `--dry-run`, `--force`, `--noisy`, `--status`, `--daemon`, and `-v` / `-vv`
  for debug output.
- Per-run Telegram status message with the run duration, time since the last successful
  update, consecutive failure count, and recovery notice.
- Per-host state file in JSON, with atomic writes and recovery from a corrupted file.
- Backoff after `abuse` responses, stored in the state file.
- Optional Prometheus textfile output (`prometheus_file`).
- Termux support: device details (model, Android version, memory, storage, battery, and
  network addresses) are added to Telegram status messages when running on Android or Termux.
- `justfile` recipes for native builds, host checks, and Termux packages, built with `cross`
  or with the Dockerfile.
- `Dockerfile` that cross-compiles for Android targets without a host Android toolchain.
- Unit tests for response classification, duration parsing and formatting, Prometheus label
  escaping, Termux battery JSON parsing, and state file round trips.
- NixOS service and timer example with a dedicated service user.

### Changed

- The source code is split into modules (`config`, `state`, `host`, `ip`, `noip`, `report`,
  `telegram`, `prometheus`, `platform`, `termux`, `util`, `log`, `cli`, `consts`, `http`).
- IP addresses are validated before they are sent. Private, loopback, link-local, multicast,
  and unspecified addresses are rejected.
- Failed updates are retried up to three times per run, with a short delay between attempts.
- TLS uses rustls through `ureq`, so no OpenSSL installation is needed.
- The release profile is optimized for binary size (`opt-level = "z"`, LTO, one codegen unit,
  stripped symbols, abort on panic).
- Status messages and console output are in English.

### Fixed

- IPv6 hosts send their address in the `myipv6` parameter instead of `myip`.
- `-v` and `-vv` now change the log output. They were previously ignored.
- The per-host `silent_on_unchanged` setting is now applied.
- The Telegram bot token is redacted from error messages.

### Removed

- `.env` configuration and the environment-variable names used by 0.1
  (`NOIP_HOSTNAME`, `NOIP_USERNAME`, and similar). See "Migration from the env-var version" in the README.
- The OpenSSL dependency.

### Notes

- The IPv6 update parameter (`myipv6`) and the response codes are based on the No-IP update
  protocol as understood during development. Verify them against the current No-IP documentation
  before relying on IPv6 hosts.
- Termux binaries are built and tested on arm64 (Xiaomi Mi 9 Lite) only. Other Android targets are
  built but not tested on hardware.

## [0.1.0]

- Initial release: single-host updates configured through environment variables, change detection
  with a state file, optional Telegram notifications, and systemd timer setup.
