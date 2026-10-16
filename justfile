# noipclient build recipes. Run `just` to list them.
# Termux builds use `cross`, which needs a running Docker daemon.

set shell := ["bash", "-euo", "pipefail", "-c"]

bin := "noipclient"
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

# Build one Termux target with Docker and package it as a .tar.gz
docker-termux target="aarch64-linux-android":
    rm -rf dist/docker/{{target}}
    docker build --target artifact --build-arg TARGET={{target}} --output type=local,dest=dist/docker/{{target}} .
    mkdir -p dist
    tar czf dist/{{bin}}-{{version}}-{{target}}.tar.gz -C dist/docker/{{target}} {{bin}} noip.toml.example
    @echo "Packaged dist/{{bin}}-{{version}}-{{target}}.tar.gz"

# Build every Termux target with the Dockerfile
docker-termux-all:
    #!/usr/bin/env bash
    set -euo pipefail
    for t in {{termux_targets}}; do
        just docker-termux "$t"
    done

# Build one Termux target with Docker, then remove ALL Docker data (runs cleanup even if the build fails)
docker-termux-clean target="aarch64-linux-android":
    #!/usr/bin/env bash
    set -uo pipefail
    status=0
    just docker-termux "{{target}}" || status=$?
    just docker-clean
    exit $status

# Remove ALL Docker data: containers, images, volumes, networks and build cache.
# Affects every Docker project on this machine, and the next build re-downloads the base image.
docker-clean:
    #!/usr/bin/env bash
    set -uo pipefail
    echo "Removing stopped containers..."
    docker container prune -f
    echo "Removing all images..."
    docker image prune -af
    echo "Removing volumes..."
    docker volume prune -f
    echo "Removing networks..."
    docker network prune -f
    echo "Removing build cache..."
    docker builder prune -af
    echo "Final full prune..."
    docker system prune -af --volumes
    echo "Removing local Docker export directories..."
    rm -rf dist/docker
    echo "Docker data removed."

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
