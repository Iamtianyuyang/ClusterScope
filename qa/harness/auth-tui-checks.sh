#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Auth-mode / agent-token / TUI checks.
#   README:249-253  agent_token on gRPC
#   README:233      auth_required: false = read-only without password
#   README:322      TUI needs -u/-p when auth_required: true
# Restarts the server and agent itself; leaves them stopped at the end.
. "$HERE/env.sh"

FAILED=0
check() {
  if [ "$2" = "$3" ]; then
    echo "CHECK $1: PASS expected=$2 actual=$3"
  else
    echo "CHECK $1: FAIL expected=$2 actual=$3"
    FAILED=1
  fi
}
node_seen() { curl -s "$HTTP/api/nodes" | jq '[.[] | select(.node_id == "'"$1"'")] | length' | awk '{print ($1>0)?1:0}'; }

mkdir -p "$QA_DIR/evidence"

# ============ 1. auth_required: true =======================================
sh "$HERE/agent-down.sh" >/dev/null
sh "$HERE/server-down.sh" >/dev/null
sh "$HERE/server-up.sh" true >/dev/null
check AUTHREQ-HEALTH-OPEN 200 "$(api_code GET /api/health)"
check AUTHREQ-NODES-NOTOKEN-401 401 "$(api_code GET /api/nodes)"
check AUTHREQ-LOGIN-OK 200 "$(api_code POST /api/login '' "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASS\"}")"
ATOK=$(login "$ADMIN_USER" "$ADMIN_PASS")
check AUTHREQ-NODES-WITH-TOKEN 200 "$(api_code GET /api/nodes "$ATOK")"
check AUTHREQ-WS-NOTOKEN-401 401 "$(curl -s -o /dev/null -w '%{http_code}' -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "$HTTP/ws")"
# README:322 — the TUI must refuse to start without credentials in this mode
TUI_OUT=$(timeout 10 "$BIN/clusterscope-tui" -s "$HTTP" 2>&1 | head -3)
echo "NOTE tui without creds: $TUI_OUT"
echo "$TUI_OUT" | grep -q 'requires authentication' && check TUI-NEEDS-LOGIN-README-322 0 0 || check TUI-NEEDS-LOGIN-README-322 0 1

# ============ 2. agent_token on gRPC ======================================
sh "$HERE/server-down.sh" >/dev/null
sh "$HERE/server-up.sh" false "qa-agent-token-0123456789" >/dev/null
sh "$HERE/agent-up.sh" qa-node-badtoken "wrong-token" >/dev/null
sleep 12
check AGENTTOKEN-WRONG-REJECTED 0 "$(node_seen qa-node-badtoken)"
echo "NOTE agent log (wrong token): $(grep -c 'invalid agent token\|Failed to register' "$QA_DIR/agent.log") error lines"
sh "$HERE/agent-down.sh" >/dev/null
sh "$HERE/agent-up.sh" qa-node-01 "qa-agent-token-0123456789" >/dev/null
sleep 12
check AGENTTOKEN-RIGHT-ACCEPTED 1 "$(node_seen qa-node-01)"

# ============ 3. TUI renders against the live server (pty) ================
# `script` gives the TUI a pty; the pty must be sized (stty) or ratatui draws
# a 0x0 frame. TERM must not be "dumb".
timeout 8 script -q -c "stty rows 40 cols 120; TERM=xterm-256color $BIN/clusterscope-tui -s $HTTP -i 1" /dev/null \
  > "$QA_DIR/evidence/tui-render.txt" 2>&1
echo "NOTE tui bytes: $(wc -c < "$QA_DIR/evidence/tui-render.txt")"
grep -q 'ClusterScope' "$QA_DIR/evidence/tui-render.txt" && check TUI-RENDERS-HEADER 0 0 || check TUI-RENDERS-HEADER 0 1
grep -qi 'panic' "$QA_DIR/evidence/tui-render.txt" && check TUI-NO-PANIC 0 1 || check TUI-NO-PANIC 0 0
grep -q 'lyy-node03' "$QA_DIR/evidence/tui-render.txt" && check TUI-SHOWS-NODE 0 0 || check TUI-SHOWS-NODE 0 1
grep -q 'q quit' "$QA_DIR/evidence/tui-render.txt" && check TUI-SHOWS-KEYHINTS 0 0 || check TUI-SHOWS-KEYHINTS 0 1

echo "----"
[ "$FAILED" -eq 0 ] && echo "AUTH/TUI-CHECKS: ALL PASS" || echo "AUTH/TUI-CHECKS: FAILURES PRESENT"
exit "$FAILED"
