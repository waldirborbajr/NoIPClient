# noip-client build recipes. Run `just` to list them.
# Termux builds use `cross`, which needs a running Docker daemon.

set shell := ["bash", "-euo", "pipefail", "-c"]

bin := "noip-client"
version := `grep -m1 '^version' Cargo.toml | cut -d'"' -f2`

# Android targets for Termux (arm64 phones, armv7 older phones, x86_64 emulators).
termux_targets := "aarch64-linux-android armv7-linux-androideabi x86_64-linux-android"

default:
    @just --list

# Native release build for this machine
build:
    cargo build --release --locked
    @echo "Built target/release/{{bin}}"

# Formatting, lints and tests on the host
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets --locked
    cargo test --locked

# Install cross if it is missing
[private]
cross-check:
    @command -v cross >/dev/null || cargo install cross --locked

# Build one Termux target and package it as dist/<bin>-<version>-<target>.tar.gz
termux target="aarch64-linux-android": cross-check
    cross build --release --locked --target {{target}}
    @just verify {{target}}
    mkdir -p dist/stage
    cp target/{{target}}/release/{{bin}} dist/stage/
    cp noip.toml.example dist/stage/
    tar czf dist/{{bin}}-{{version}}-{{target}}.tar.gz -C dist/stage {{bin}} noip.toml.example
    rm -rf dist/stage
    @echo "Packaged dist/{{bin}}-{{version}}-{{target}}.tar.gz"

# Build every Termux target
termux-all:
    #!/usr/bin/env bash
    set -euo pipefail
    for t in {{termux_targets}}; do
        just termux "$t"
    done

# Check that a built binary is an Android ELF for the expected architecture
[private]
verify target:
    #!/usr/bin/env bash
    set -euo pipefail
    f="target/{{target}}/release/{{bin}}"
    test -x "$f" || { echo "error: $f missing"; exit 1; }
    info=$(file -b "$f")
    case "{{target}}" in
        aarch64-*)  grep -q 'ARM aarch64' <<<"$info" || { echo "error: wrong arch: $info"; exit 1; } ;;
        armv7-*)    grep -q 'ARM,' <<<"$info"        || { echo "error: wrong arch: $info"; exit 1; } ;;
        x86_64-*)   grep -q 'x86-64' <<<"$info"      || { echo "error: wrong arch: $info"; exit 1; } ;;
    esac
    echo "ok: $info"

# Remove build output
clean:
    cargo clean
    rm -rf dist
