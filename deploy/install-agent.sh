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
# Generate the config *on the remote host* so ~ expands to the remote HOME.
ssh -o BatchMode=yes "$TARGET" "NODE_ID='$NODE_ID' SERVER_ADDR='$SERVER_ADDR' bash -s" <<'REMOTE'
set -e
mkdir -p ~/.config/clusterscope
cat > ~/.config/clusterscope/agent.yaml <<EOF
server_addr: "$SERVER_ADDR"
node_id: "$NODE_ID"
node_id_file: ~/.config/clusterscope/node_id
report_interval_secs: 2
log_dir: ~/.config/clusterscope/logs
log_level: info
disk_mounts:
  - /
EOF
REMOTE

echo "==> starting agent on $TARGET"
ssh "$TARGET" 'bash -s' <<'EOF'
set -e
mkdir -p ~/.config/clusterscope/logs
if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
    mkdir -p ~/.config/systemd/user
    cat > ~/.config/systemd/user/clusterscope-agent.service <<SVC
[Unit]
Description=ClusterScope GPU Agent
After=network-online.target

[Service]
Type=simple
ExecStart=$HOME/.local/bin/clusterscope-agent -c $HOME/.config/clusterscope/agent.yaml
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
SVC
    systemctl --user daemon-reload
    systemctl --user enable --now clusterscope-agent.service
    echo "started via systemd --user"
else
    pkill -f clusterscope-agent 2>/dev/null || true
    nohup "$HOME/.local/bin/clusterscope-agent" -c "$HOME/.config/clusterscope/agent.yaml" \
        >> "$HOME/.config/clusterscope/logs/agent.log" 2>&1 &
    echo "started via nohup"
fi
EOF

echo "==> done. Check the dashboard: node '$NODE_ID' should appear within ~10s."
