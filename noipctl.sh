#!/bin/sh
# noipctl: start, stop, restart and inspect the noipclient daemon.
# Works on Termux (Android) and on Linux. Runs the daemon in the background
# with `&` (no nohup, no cron).
#
# Usage: noipctl.sh {start|stop|restart|status|log}
#
# Defaults expect the binary and config in $HOME/noip. Override with:
#   NOIP_DIR      directory with the binary, config, log and pid file (default: $HOME/noip)
#   NOIP_BIN      path to the binary (default: $NOIP_DIR/noipclient)
#   NOIP_CONFIG   path to the config (default: $NOIP_DIR/noip.toml)
#   NOIP_LOG      path to the log (default: $NOIP_DIR/noip.log)
#   NOIP_PIDFILE  path to the pid file (default: $NOIP_DIR/noipclient.pid)
#
# Example on a notebook, using the binary built with `cargo build --release`:
#   NOIP_BIN=./target/release/noipclient NOIP_CONFIG=./noip.toml NOIP_DIR=/tmp/noip ./noipctl.sh start

set -eu

DIR="${NOIP_DIR:-$HOME/noip}"
BIN="${NOIP_BIN:-$DIR/noipclient}"
CONFIG="${NOIP_CONFIG:-$DIR/noip.toml}"
LOG="${NOIP_LOG:-$DIR/noip.log}"
PIDFILE="${NOIP_PIDFILE:-$DIR/noipclient.pid}"

is_running() {
    [ -f "$PIDFILE" ] || return 1
    kill -0 "$(cat "$PIDFILE")" 2>/dev/null
}

start() {
    if is_running; then
        echo "already running (pid $(cat "$PIDFILE"))"
        return 0
    fi
    [ -x "$BIN" ] || { echo "error: $BIN not found or not executable" >&2; exit 1; }
    [ -f "$CONFIG" ] || { echo "error: config $CONFIG not found" >&2; exit 1; }

    rm -f "$PIDFILE"
    "$BIN" --config "$CONFIG" --daemon >>"$LOG" 2>&1 &
    echo $! > "$PIDFILE"

    sleep 1
    if is_running; then
        echo "started (pid $(cat "$PIDFILE")), log: $LOG"
    else
        rm -f "$PIDFILE"
        echo "error: daemon exited right after start. Last log lines:" >&2
        tail -n 20 "$LOG" 2>/dev/null >&2 || true
        exit 1
    fi
}

stop() {
    if ! is_running; then
        echo "not running"
        rm -f "$PIDFILE"
        return 0
    fi
    pid=$(cat "$PIDFILE")
    kill "$pid"
    i=0
    while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 10 ]; do
        sleep 1
        i=$((i + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" 2>/dev/null || true
    fi
    rm -f "$PIDFILE"
    echo "stopped (pid $pid)"
}

status() {
    if is_running; then
        echo "running (pid $(cat "$PIDFILE"))"
    else
        echo "not running"
    fi
}

case "${1:-}" in
    start)   start ;;
    stop)    stop ;;
    restart) stop; start ;;
    status)  status ;;
    log)     tail -n 50 -f "$LOG" ;;
    *)
        echo "usage: $0 {start|stop|restart|status|log}" >&2
        exit 2
        ;;
esac
