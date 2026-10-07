#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Stop the harness agent started by agent-up.sh (PID file only — never pkill).
. "$HERE/env.sh"

if [ -f "$QA_DIR/agent.pid" ]; then
  pid=$(cat "$QA_DIR/agent.pid")
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null
    i=0
    while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 10 ]; do i=$((i + 1)); sleep 1; done
    kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null
    echo "agent stopped (pid $pid)"
  else
    echo "agent not running (stale pid $pid)"
  fi
  rm -f "$QA_DIR/agent.pid"
else
  echo "no agent.pid — nothing to stop"
fi
