#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Start clusterscope-agent against the local server.
#   usage: agent-up.sh [node_id] [agent_token]
# node_id "" (empty) exercises the documented "empty = use local hostname" path.
# The host has 6 real NVIDIA L20 GPUs and a working nvidia-smi, so the agent
# reports real GPU metrics (NVML may be absent; the nvidia-smi fallback covers it).
set -e
. "$HERE/env.sh"

NID="${1-$NODE_ID}"
ATOK="${2:-}"
LOGDIR="$QA_DIR/agent-logs"
mkdir -p "$LOGDIR"

cat > "$QA_DIR/agent.yaml" <<EOF
server_addr: "http://$GRPC_ADDR"
node_id: "$NID"
node_id_file: $QA_DIR/node_id
report_interval_secs: 2
log_dir: $LOGDIR
log_level: info
collect_process_details: true
agent_token: "$ATOK"
EOF

if [ -f "$QA_DIR/agent.pid" ] && kill -0 "$(cat "$QA_DIR/agent.pid")" 2>/dev/null; then
  echo "agent already running (pid $(cat "$QA_DIR/agent.pid"))"
  exit 0
fi

: > "$QA_DIR/agent.log"
nohup "$BIN/clusterscope-agent" -c "$QA_DIR/agent.yaml" >> "$QA_DIR/agent.log" 2>&1 &
echo $! > "$QA_DIR/agent.pid"
echo "agent started (pid $(cat "$QA_DIR/agent.pid")), config $QA_DIR/agent.yaml, log $QA_DIR/agent.log"
