#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Stop the harness server started by server-up.sh (PID file only — never pkill).
. "$HERE/env.sh"

if [ -f "$QA_DIR/server.pid" ]; then
  pid=$(cat "$QA_DIR/server.pid")
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null
    i=0
    while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 10 ]; do i=$((i + 1)); sleep 1; done
    kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null
    echo "server stopped (pid $pid)"
  else
    echo "server not running (stale pid $pid)"
  fi
  rm -f "$QA_DIR/server.pid"
else
  echo "no server.pid — nothing to stop"
fi
