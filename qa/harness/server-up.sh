#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Start clusterscope-server against the local PostgreSQL with a known config.
#   usage: server-up.sh [auth_required:true|false] [agent_token]
# Writes $QA_DIR/server.yaml, starts the binary detached, waits for /api/health.
# The PID is recorded in $QA_DIR/server.pid so stop only ever kills our own
# process (this host is shared — never pkill by name).
set -e
. "$HERE/env.sh"

AUTH="${1:-false}"
TOKEN="${2:-}"

pg_start

cat > "$QA_DIR/server.yaml" <<EOF
grpc_addr: "$GRPC_ADDR"
http_addr: "127.0.0.1:8080"
postgres_url: "$PGURL"
jwt_secret: "qa-harness-secret-0123456789abcdef"
default_admin_username: "$ADMIN_USER"
default_admin_password: "$ADMIN_PASS"
auth_required: $AUTH
agent_token: "$TOKEN"
EOF

if [ -f "$QA_DIR/server.pid" ] && kill -0 "$(cat "$QA_DIR/server.pid")" 2>/dev/null; then
  echo "server already running (pid $(cat "$QA_DIR/server.pid"))"
  exit 0
fi

: > "$QA_DIR/server.log"
nohup "$BIN/clusterscope-server" "$QA_DIR/server.yaml" >> "$QA_DIR/server.log" 2>&1 &
echo $! > "$QA_DIR/server.pid"

if wait_http "$HTTP/api/health" 30; then
  echo "server up (pid $(cat "$QA_DIR/server.pid")), config $QA_DIR/server.yaml, log $QA_DIR/server.log"
else
  echo "SERVER FAILED TO START — last log lines:"
  tail -20 "$QA_DIR/server.log"
  exit 1
fi
