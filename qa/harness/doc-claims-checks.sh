#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Documentation-vs-implementation checks (static, no server needed).
# Every claim cites the doc line it comes from; every expectation cites the code.
# Prints "CHECK <id>: PASS|FAIL detail"; exit 1 if any FAIL.
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
has() { grep -q "$1" "$2"; }

# --- licence -----------------------------------------------------------------
# README:19 badge + README:363 link -> blob/master/LICENSE; Cargo.toml:14 declares Apache-2.0
test -f "$REPO/LICENSE" && check DOC-LICENSE-FILE 0 0 || check DOC-LICENSE-FILE 0 1
check DOC-LICENSE-DECLARED 1 "$(grep -c 'license = "Apache-2.0"' "$REPO/Cargo.toml" | awk '{print ($1>0)?1:0}')"
check DOC-LICENSE-LINK 1 "$(grep -c 'blob/master/LICENSE' "$REPO/README.md" | awk '{print ($1>0)?1:0}')"

# --- retention policy: README:301-311 vs architecture.md:62 vs code ----------
# code: crates/server/src/handlers.rs:303-306 (24h raw / 7d hourly), 90d daily and
#       30d job logs in crates/server/src/main.rs:345-376, crates/storage/src/queries.rs:217-226
check DOC-CODE-RAW-RETENTION-24H 1 "$(grep -c 'RAW_RETENTION_MS: i64 = 24 \* 3600 \* 1000' "$REPO/crates/server/src/handlers.rs" | awk '{print ($1>0)?1:0}')"
check DOC-CODE-HOURLY-RETENTION-7D 1 "$(grep -c 'HOURLY_RETENTION_MS: i64 = 7 \* 24' "$REPO/crates/server/src/handlers.rs" | awk '{print ($1>0)?1:0}')"
check DOC-CODE-DAILY-CLEANUP-90 1 "$(grep -c 'cleanup_daily_data(state.database.pool(), 90)' "$REPO/crates/server/src/main.rs" | awk '{print ($1>0)?1:0}')"
check DOC-CODE-JOBLOG-RETENTION-30D 1 "$(grep -c 'ChronoDuration::days(30)' "$REPO/crates/server/src/main.rs" | awk '{print ($1>0)?1:0}')"
check DOC-README-RETENTION-TABLE 1 "$(grep -c '| 原始指标(2s 粒度) | 24 小时 |' "$REPO/README.md" | awk '{print ($1>0)?1:0}')"
# architecture.md:62 still describes the abandoned 2s -> 1min -> 10min policy
check DOC-ARCH-RETENTION-MATCHES-CODE 0 "$(grep -c '2s → 1min → 10min' "$REPO/docs/architecture.md" | awk '{print ($1>0)?1:0}')"

# --- api.md endpoint list vs the real router (main.rs:234-312) ---------------
for ep in '/api/login' '/api/refresh-token' '/api/nodes' '/api/nodes/{node_id}' \
          '/api/nodes/{node_id}/metrics' '/api/metrics/history' '/api/jobs' \
          '/api/jobs/{job_id}' '/api/jobs/{job_id}/logs' '/api/alerts/rules' \
          '/api/alerts/rules/{rule_id}' '/api/alerts/events' '/api/cluster/info' \
          '/api/users' '/api/audit-logs' '/ws'; do
  base=$(echo "$ep" | sed 's|/api||')
  has "$base" "$REPO/crates/server/src/main.rs" && r=1 || r=0
  has "$ep" "$REPO/docs/api.md" && d=1 || d=0
  check "DOC-API-EP-DOCUMENTED-AND-ROUTED $(echo "$ep" | tr -c 'a-zA-Z0-9' '_')" 11 "$r$d"
done
# routes that exist but are missing from api.md (documented gaps)
for ep in '/api/health' '/api/users/{id}' '/api/alerts/rules/{rule_id}/state' '/api/prometheus/metrics'; do
  has "$ep" "$REPO/docs/api.md" && d=1 || d=0
  check "DOC-API-EP-IN-DOC $(echo "$ep" | tr -c 'a-zA-Z0-9' '_')" 1 "$d"
done

# --- README:154-160 TUI key bindings vs crates/tui/src/main.rs:85-140 --------
for k in "'j'" "'k'" "'p'" "'h'" "'l'" "KeyCode::Tab" "'1'" "'2'" "'3'" "'4'" "'r'" "'?'" "'q'"; do
  has "$k" "$REPO/crates/tui/src/main.rs" && check "DOC-TUI-KEY $k" 0 0 || check "DOC-TUI-KEY $k" 0 1
done
# README:117-125 TUI flags and defaults
check DOC-TUI-FLAG-DEFAULT-SERVER 1 "$(grep -c 'default_value = "http://127.0.0.1:8080"' "$REPO/crates/tui/src/main.rs" | awk '{print ($1>0)?1:0}')"
check DOC-TUI-FLAG-DEFAULT-INTERVAL 1 "$(grep -c 'default_value_t = 3' "$REPO/crates/tui/src/main.rs" | awk '{print ($1>0)?1:0}')"
check DOC-TUI-FLAGS-U-P 1 "$(grep -c 'username: Option<String>' "$REPO/crates/tui/src/main.rs" | awk '{print ($1>0)?1:0}')"

# --- README:355 "cancellation escalates to SIGKILL when force is configured" -
check DOC-README-SIGKILL-CLAIM 1 "$(grep -c 'SIGKILL' "$REPO/README.md" | awk '{print ($1>0)?1:0}')"
check DOC-CODE-SIGKILL-EXISTS 1 "$(grep -rc 'SIGKILL' "$REPO/crates" --include='*.rs' | awk -F: '{s+=$2} END {print (s>0)?1:0}')"
check DOC-CODE-FORCE-OPTION 1 "$(grep -c '"force"' "$REPO/crates" -r --include='*.rs' | awk -F: '{s+=$2} END {print (s>0)?1:0}')"

# --- README:249-253 / deploy/docker-compose.yml env vars are really read -----
for v in POSTGRES_URL JWT_SECRET AUTH_REQUIRED AGENT_TOKEN HTTP_ADDR GRPC_ADDR; do
  has "$v" "$REPO/crates/server/src/main.rs" && check "DOC-ENV-READ $v" 0 0 || check "DOC-ENV-READ $v" 0 1
done

# --- deploy/*.yaml.example keys must be real config fields ------------------
for k in server_addr node_id node_id_file report_interval_secs log_dir log_level disk_mounts collect_process_details agent_token; do
  has "$k" "$REPO/crates/common/src/config.rs" && check "DOC-AGENT-YAML-KEY $k" 0 0 || check "DOC-AGENT-YAML-KEY $k" 0 1
done
for k in grpc_addr http_addr postgres_url jwt_secret default_admin_username default_admin_password auth_required agent_token; do
  has "$k" "$REPO/crates/common/src/config.rs" && check "DOC-SERVER-YAML-KEY $k" 0 0 || check "DOC-SERVER-YAML-KEY $k" 0 1
done
# README:232 documents default_admin_password "admin123"; the serde default is "admin"
check DOC-ADMIN-PASSWORD-DEFAULT 1 "$(grep -c 'default_admin_password: "admin123"' "$REPO/README.md" | awk '{print ($1>0)?1:0}')"
check DOC-ADMIN-PASSWORD-CODE-DEFAULT 1 "$(grep -c 'default_admin_password: "admin".to_string()' "$REPO/crates/common/src/config.rs" | awk '{print ($1>0)?1:0}')"

# --- README:57 / deploy/docker-compose.yml PostgreSQL version ----------------
check DOC-COMPOSE-PG16 1 "$(grep -c 'image: postgres:16-alpine' "$REPO/deploy/docker-compose.yml" | awk '{print ($1>0)?1:0}')"
check DOC-DOCKERFILE-EXPOSE-DEAD-PORTS 1 "$(grep -c 'EXPOSE 8080 8081 50051 9090' "$REPO/deploy/Dockerfile.server" | awk '{print ($1>0)?1:0}')"

# --- deploy/install-agent.sh + systemd units must use flags the binaries have -
check DOC-INSTALL-AGENT-USES-C 1 "$(grep -c '\-c \$HOME/.config/clusterscope/agent.yaml' "$REPO/deploy/install-agent.sh" | awk '{print ($1>0)?1:0}')"
check DOC-AGENT-UNIT-USES-CONFIG-FLAG 1 "$(grep -c '\-\-config /etc/clusterscope/agent.yaml' "$REPO/deploy/agent.service" | awk '{print ($1>0)?1:0}')"
check DOC-SERVER-UNIT-PASSES-CONFIG 1 "$(grep -c 'ExecStart=/usr/local/bin/clusterscope-server /etc/clusterscope/server.yaml' "$REPO/deploy/server.service" | awk '{print ($1>0)?1:0}')"
# install-agent.sh hardcodes the metrics report interval; README:243 says 2s
check DOC-INSTALL-AGENT-REPORT-INTERVAL 1 "$(grep -c 'report_interval_secs: 2' "$REPO/deploy/install-agent.sh" | awk '{print ($1>0)?1:0}')"

echo "----"
[ "$FAILED" -eq 0 ] && echo "DOC-CHECKS: ALL PASS" || echo "DOC-CHECKS: FAILURES PRESENT"
exit "$FAILED"
