#!/bin/bash
# Install clusterscope-agent on another host via passwordless SSH, no root needed.
#
# Usage:
#   ./install-agent.sh user@host <server-addr> [node-id]
#
# Examples:
#   ./install-agent.sh worker1@192.168.1.20 http://203.0.113.1:50051
#   ./install-agent.sh worker2@192.168.1.21 http://203.0.113.1:50051 gpu-node-2
#
# The agent is installed under ~/.local/bin and ~/.config/clusterscope on the
# remote host and started via systemd --user (or nohup as a fallback).
# Server must accept agents from this host (no auth required by default).
#
# Stopping/replacing an agent: this script only ever stops the process it
# started itself. The nohup fallback records that child PID in
# ~/.config/clusterscope/agent.pid and re-checks /proc/<pid>/cmdline before
# signalling it, so other agents of the same user on a shared machine -- a
# systemd unit, a manual run, an install done by someone else -- are never
# touched. Matching processes by name is deliberately not done here.

set -euo pipefail

TARGET="${1:?usage: install-agent.sh user@host <server-addr> [node-id]}"
SERVER_ADDR="${2:?server-addr is required, e.g. http://203.0.113.1:50051}"
NODE_ID="${3:-}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/clusterscope-agent"
[ -f "$BIN" ] || BIN="$ROOT/target/debug/clusterscope-agent"
[ -f "$BIN" ] || { echo "build the agent first: cargo build --release -p agent"; exit 1; }

echo "==> testing passwordless ssh to $TARGET"
ssh -o BatchMode=yes "$TARGET" 'echo ok' >/dev/null

echo "==> uploading agent binary"
ssh -o BatchMode=yes "$TARGET" 'mkdir -p ~/.local/bin ~/.config/clusterscope'
# Copy to /tmp first, then move: overwriting a running binary in place can fail.
scp -q "$BIN" "$TARGET:/tmp/clusterscope-agent.new"
ssh -o BatchMode=yes "$TARGET" 'mv -f /tmp/clusterscope-agent.new ~/.local/bin/clusterscope-agent && chmod +x ~/.local/bin/clusterscope-agent'

# Default: empty node_id -> each agent uses its own local hostname.
# (Works on shared-HOME clusters where one config file is shared by all nodes.)
: "${NODE_ID:=}"

echo "==> writing config (node_id=$NODE_ID, server=$SERVER_ADDR)"
# Generate the config *on the remote host* so $HOME expands to the remote HOME.
# Absolute paths on purpose: the agent does not expand "~", so a "~/.config/..."
# value would be resolved against the process working directory instead of HOME.
ssh -o BatchMode=yes "$TARGET" "NODE_ID='$NODE_ID' SERVER_ADDR='$SERVER_ADDR' bash -s" <<'REMOTE'
set -e
mkdir -p ~/.config/clusterscope
cat > ~/.config/clusterscope/agent.yaml <<EOF
server_addr: "$SERVER_ADDR"
node_id: "$NODE_ID"
node_id_file: $HOME/.config/clusterscope/node_id
report_interval_secs: 2
log_dir: $HOME/.config/clusterscope/logs
log_level: info
disk_mounts:
  - /
EOF
REMOTE

echo "==> starting agent on $TARGET"
ssh "$TARGET" 'bash -s' <<'EOF'
set -eu
mkdir -p ~/.config/clusterscope/logs
BIN="$HOME/.local/bin/clusterscope-agent"
CONF="$HOME/.config/clusterscope/agent.yaml"
PIDFILE="$HOME/.config/clusterscope/agent.pid"

# PID-based stop: only the agent this script started in an earlier run is ever
# stopped. The pid comes from our own pidfile, and /proc/<pid>/cmdline must
# still show this exact binary+config pair before we signal it -- a stale or
# reused pid is left alone. (Name-based process matching would hit every agent
# of this user on a shared machine, so it is not used here.)
stop_previous_agent() {
    [ -f "$PIDFILE" ] || return 0
    pid="$(cat "$PIDFILE" 2>/dev/null || true)"
    rm -f "$PIDFILE"
    case "$pid" in
        '' | *[!0-9]*) return 0 ;;
    esac
    if [ ! -r "/proc/$pid/cmdline" ]; then
        echo "previous agent (pid $pid) is already gone"
        return 0
    fi
    cmdline="$(tr '\0' ' ' < "/proc/$pid/cmdline" || true)"
    case "$cmdline" in
        *"$BIN"*"$CONF"*)
            echo "stopping previous agent (pid $pid)"
            kill "$pid" 2>/dev/null || true
            waited=0
            while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 20 ]; do
                sleep 0.5
                waited=$((waited + 1))
            done
            ;;
        *)
            echo "pidfile points at an unrelated process (pid $pid) -- leaving it alone"
            ;;
    esac
}

if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
    stop_previous_agent
    mkdir -p ~/.config/systemd/user
    cat > ~/.config/systemd/user/clusterscope-agent.service <<SVC
[Unit]
Description=ClusterScope GPU Agent
After=network-online.target

[Service]
Type=simple
ExecStart=$BIN -c $CONF
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
SVC
    systemctl --user daemon-reload
    systemctl --user enable --now clusterscope-agent.service
    echo "started via systemd --user"
else
    stop_previous_agent
    nohup "$BIN" -c "$CONF" >> "$HOME/.config/clusterscope/logs/agent.log" 2>&1 &
    echo "$!" > "$PIDFILE"
    echo "started via nohup (pid $(cat "$PIDFILE"))"
fi
EOF

echo "==> done. Check the dashboard: node '$NODE_ID' should appear within ~10s."
