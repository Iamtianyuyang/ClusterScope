#!/bin/bash
# ClusterScope TUI launcher — opens the terminal dashboard without a password.
#
# Requirements:
#   - clusterscope-tui binary on PATH (or set TUI_BIN below)
#   - server.yaml has `auth_required: false` (read-only mode)
#
# Usage:
#   ./tui.sh [server-url]          e.g. ./tui.sh http://127.0.0.1:8080
#   ./tui.sh --install             copy binary to ~/.local/bin and print ssh tip

set -euo pipefail

SERVER="${1:-http://127.0.0.1:8080}"
SESSION="clusterscope"
TUI_BIN="${TUI_BIN:-clusterscope-tui}"
SRC_BIN="$(cd "$(dirname "$0")/.." && pwd)/target/debug/clusterscope-tui"

if [ "${1:-}" = "--install" ]; then
    mkdir -p "$HOME/.local/bin"
    cp -f "$SRC_BIN" "$HOME/.local/bin/clusterscope-tui"
    echo "installed: $HOME/.local/bin/clusterscope-tui"
    echo "ssh tip: add to ~/.bashrc to auto-open the dashboard on login:"
    echo '  if [ -z "$TMUX" ] && [ -n "$PS1" ]; then clusterscope-tui; fi'
    exit 0
fi

if ! command -v "$TUI_BIN" >/dev/null 2>&1; then
    TUI_BIN="$SRC_BIN"
fi

echo "connecting to $SERVER (no password needed in read-only mode)"

if command -v tmux >/dev/null 2>&1; then
    # Reuse an existing session if present, otherwise create one.
    exec tmux new-session -A -s "$SESSION" "$TUI_BIN -s $SERVER"
else
    exec "$TUI_BIN" -s "$SERVER"
fi
