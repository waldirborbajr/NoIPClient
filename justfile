# =============================================
# noipclient - justfile (build, quality, Termux, Docker)
# Run `just` to list all recipes.
# Termux builds use `cross` (needs Docker) or the Dockerfile (needs Docker).
# =============================================

set shell := ["bash", "-euo", "pipefail", "-c"]

bin := "noipclient"
version := `grep -m1 '^version' Cargo.toml | cut -d'"' -f2`

# Android targets for Termux (arm64 phones, armv7 older phones, x86_64 emulators).
termux_targets := "aarch64-linux-android armv7-linux-androideabi x86_64-linux-android"

# Default command
default:
    @just --list

# === Build & Run ===

# Native release build for this machine
build:
    cargo build --release --locked
    @echo "Built target/release/{{bin}}"

# Debug build
build-debug:
    cargo build --locked

# Run in debug mode (lints first)
run: clippy
    cargo run

# Run in release mode (lints first)
run-release: clippy
    cargo run --release

# === Quality & Linting ===

# Format the code with rustfmt
fmt:
    cargo fmt --all

# Check formatting without changing files (fails if anything needs formatting)
fmt-check:
    cargo fmt --all -- --check

# Run clippy on all targets and features; any warning fails the recipe
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Apply clippy suggestions automatically, then re-check with -D warnings
fix:
    cargo clippy --fix --allow-dirty --allow-staged --all-targets --all-features -- -D warnings

# Formatting, lints and tests on the host
check: fmt-check clippy test-std

# === Testing ===

# Run tests with cargo-nextest (install: cargo install cargo-nextest --locked)
test:
    cargo nextest run

# Verbose test run with cargo-nextest
test-verbose:
    cargo nextest run --verbose

# Tests with the standard harness (no extra tool needed)
test-std:
    cargo test --locked

# Everything: nextest suite plus doc tests
test-all: test
    cargo test --doc

# === Audit & Security ===

# Check dependencies for known vulnerabilities (install: cargo install cargo-audit --locked)
audit:
    cargo audit

# Show outdated dependencies (install: cargo install cargo-outdated --locked)
outdated:
    cargo outdated

# Find unused dependencies (needs nightly; install: cargo install cargo-udeps --locked)
udeps:
    cargo +nightly udeps --all-targets

# === Code Analysis ===

# Expand macros (install: cargo install cargo-expand --locked)
expand *ARGS:
    cargo expand {{ARGS}}

# Flame graph of the binary (install: cargo install flamegraph --locked)
flamegraph *ARGS:
    cargo flamegraph {{ARGS}}

# Show which functions generate the most LLVM IR (install: cargo install cargo-llvm-lines --locked)
llvm-lines:
    cargo llvm-lines --release | head -n 30

# Coverage report in HTML (install: cargo install cargo-tarpaulin --locked)
tarpaulin:
    cargo tarpaulin --ignore-tests --out Html

# Size per crate in the release binary (install: cargo install cargo-bloat --locked)
size:
    cargo bloat --release --crates

# === Dependencies ===

# Update Cargo.lock within the existing version requirements
update:
    cargo update

# Add a dependency, e.g.: just add serde --features derive
add *ARGS:
    cargo add {{ARGS}}

# Remove a dependency
rm *ARGS:
    cargo remove {{ARGS}}

# === Release & Publishing ===

# Check for breaking API changes (install: cargo install cargo-semver-checks --locked)
semver-check:
    cargo semver-checks check-release

# Package and verify for crates.io without publishing
publish-dry:
    cargo publish --dry-run

# === Utils ===

# Re-run a command on file changes (install: cargo install cargo-watch --locked)
watch +ARGS="run":
    cargo watch -x "{{ARGS}}"

# === Termux builds (cross) ===

# Install cross if it is missing
[private]
cross-check:
    @command -v cross >/dev/null || cargo install cross --locked

# Build one Termux target with cross and package it as dist/<bin>-<version>-<target>.tar.gz
termux target="aarch64-linux-android": cross-check
    cross build --release --locked --target {{target}}
    @just verify {{target}}
    mkdir -p dist/stage
    cp target/{{target}}/release/{{bin}} dist/stage/
    cp noip.toml.example dist/stage/
    tar czf dist/{{bin}}-{{version}}-{{target}}.tar.gz -C dist/stage {{bin}} noip.toml.example
    rm -rf dist/stage
    @echo "Packaged dist/{{bin}}-{{version}}-{{target}}.tar.gz"

# Build every Termux target with cross
termux-all:
    #!/usr/bin/env bash
    set -euo pipefail
    for t in {{termux_targets}}; do
        just termux "$t"
    done

# === Termux builds (Docker) ===

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

# Build one Termux target with Docker, then remove ALL Docker data (cleanup runs even if the build fails)
docker-termux-clean target="aarch64-linux-android":
    #!/usr/bin/env bash
    set -uo pipefail
    status=0
    just docker-termux "{{target}}" || status=$?
    just docker-clean
    exit $status

# Remove ALL Docker data: containers, images, volumes, networks and build cache.
# Affects every Docker project on this machine; the next build re-downloads the base image.
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

# === Full check ===

# Everything that must pass before a release
check-all: clippy test-std fmt-check audit outdated
    @echo "All checks passed!"

# === Cleanup ===

# Remove build output (cargo target/ and dist/)
clean:
    cargo clean
    rm -rf dist
