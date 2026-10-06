#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Runtime / ops checks for ClusterScope: CLI surface, listening ports, Prometheus
# endpoint, migration idempotency, retention pruning, history tier merge, and the
# static "dead config key" scan.
# Preconditions: server up (`sh qa/harness/server-up.sh false`), agent up
# (`sh qa/harness/agent-up.sh`), PostgreSQL running.
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

mkdir -p "$QA_DIR/evidence"

# --- CLI surface ------------------------------------------------------------
"$BIN/clusterscope-agent" --help > "$QA_DIR/evidence/help-agent.txt" 2>&1
check CLI-HELP-AGENT-EXIT 0 "$?"
grep -q -- '--config' "$QA_DIR/evidence/help-agent.txt"
check CLI-HELP-AGENT-MENTIONS-CONFIG 0 "$?"

"$BIN/clusterscope-tui" --help > "$QA_DIR/evidence/help-tui.txt" 2>&1
check CLI-HELP-TUI-EXIT 0 "$?"
grep -q -- '--server' "$QA_DIR/evidence/help-tui.txt"
check CLI-HELP-TUI-MENTIONS-SERVER 0 "$?"

"$BIN/clusterscope-server" --help > "$QA_DIR/evidence/help-server.txt" 2>&1
SERVER_HELP_EXIT=$?
echo "NOTE server --help exit=$SERVER_HELP_EXIT output: $(head -1 "$QA_DIR/evidence/help-server.txt")"
check CLI-HELP-SERVER-EXIT 0 "$SERVER_HELP_EXIT"
"$BIN/clusterscope-server" /nonexistent/qa.yaml > "$QA_DIR/evidence/server-badcfg.txt" 2>&1
check CLI-SERVER-MISSING-CONFIG-EXIT 1 "$?"

# --- listening ports --------------------------------------------------------
PORTS=$(ss -ltn 2>/dev/null | awk 'NR>1 {print $4}' | sed 's/.*://' | sort -u | tr '\n' ' ')
echo "NOTE listening ports: $PORTS"
echo "$PORTS" | grep -q ' 8080 ' && check PORT-8080 0 0 || check PORT-8080 0 1
echo "$PORTS" | grep -q ' 50051 ' && check PORT-50051 0 0 || check PORT-50051 0 1
echo "$PORTS" | grep -q ' 9090 ' && check PORT-9090-NOT-LISTENING 0 1 || check PORT-9090-NOT-LISTENING 0 0

# --- Prometheus endpoint (served from the REST port, not prometheus_addr) ---
PROM=$(api_get /api/prometheus/metrics)
echo "$PROM" > "$QA_DIR/evidence/prometheus.txt"
echo "$PROM" | grep -q 'nodes_total' && check PROM-METRIC-NODES-TOTAL 0 0 || check PROM-METRIC-NODES-TOTAL 0 1
echo "$PROM" | grep -q 'nodes_online' && check PROM-METRIC-NODES-ONLINE 0 0 || check PROM-METRIC-NODES-ONLINE 0 1

# --- migration idempotency: a second start must not duplicate the admin -----
ADMIN_BEFORE=$(psql_q "select count(*) from users where username='admin';")
sh "$HERE/server-down.sh" >/dev/null
sh "$HERE/server-up.sh" false >/dev/null
check MIGRATION-RESTART-HEALTHY 200 "$(api_code GET /api/health)"
check MIGRATION-ADMIN-SINGLE 1 "$(psql_q "select count(*) from users where username='admin';")"
echo "NOTE admin rows before restart=$ADMIN_BEFORE after=$(psql_q "select count(*) from users where username='admin';")"

# --- README:296 "agents re-register every 60s after a server restart" -------
i=0
while [ "$i" -lt 75 ]; do
  n=$(curl -s "$HTTP/api/nodes" | jq '[.[] | select(.node_id == "'"$NODE_ID"'")] | length')
  [ "$n" != "0" ] && break
  i=$((i + 1)); sleep 1
done
echo "NOTE node re-appeared after ${i}s"
check NODE-REAPPEARS-AFTER-RESTART 1 "$(curl -s "$HTTP/api/nodes" | jq '[.[] | select(.node_id == "'"$NODE_ID"'")] | length' | awk '{print ($1>0)?1:0}')"

# --- README:217 "metrics are collected every 2s" ---------------------------
sleep 20
ROWS=$(psql_q "select count(*) from node_metrics where node_id='$NODE_ID' and created_at > now() - interval '20 seconds';")
check AGENT-REPORT-CADENCE-2S 1 "$(echo "$ROWS" | awk '{print ($1 >= 7 && $1 <= 13) ? 1 : 0}')"
echo "NOTE node_metrics rows in the last 20s: $ROWS (2s cadence => ~10)"

# --- retention: raw metrics older than 24h are pruned (background tick 10s) -
NOW_MS=$(python3 -c 'import time;print(int(time.time()*1000))')
OLD_MS=$((NOW_MS - 25 * 3600 * 1000))
FRESH_MS=$((NOW_MS - 3600 * 1000))
SEQ_OLD=$((NOW_MS + 1))
SEQ_FRESH=$((NOW_MS + 2))
psql_q "insert into node_metrics (node_id, sequence, timestamp_ms, cpu_usage_percent) values ('$NODE_ID', $SEQ_OLD, $OLD_MS, 1.0), ('$NODE_ID', $SEQ_FRESH, $FRESH_MS, 2.0);" >/dev/null
sleep 20
check RETENTION-RAW-OLD-PRUNED 0 "$(psql_q "select count(*) from node_metrics where sequence=$SEQ_OLD;")"
check RETENTION-RAW-FRESH-KEPT 1 "$(psql_q "select count(*) from node_metrics where sequence=$SEQ_FRESH;")"

# --- history tier merge (hourly 7d / daily 90d read path) -------------------
# Fixture note: day_bucket is a DATE, so the bucket of the exact start day is
# excluded by the `day_bucket >= to_timestamp(start)` bound (time-of-day
# comparison). The daily fixture therefore sits 29 days back, not exactly 30.
DAY_MS=86400000
H3=$((NOW_MS - 3 * DAY_MS))
D29=$((NOW_MS - 29 * DAY_MS))
D30=$((NOW_MS - 30 * DAY_MS))
psql_q "insert into metrics_hourly (node_id, metric_name, hour_bucket, avg_value, max_value, min_value, p95_value, sample_count)
        values ('$NODE_ID','cpu_usage_percent', to_timestamp($H3/1000.0), 11.0, 12.0, 10.0, 11.5, 5)
        on conflict (node_id, metric_name, hour_bucket) do update set avg_value = excluded.avg_value;" >/dev/null
psql_q "insert into metrics_daily (node_id, metric_name, day_bucket, avg_value, max_value, min_value, p95_value, sample_count)
        values ('$NODE_ID','cpu_usage_percent', (to_timestamp($D29/1000.0))::date, 21.0, 22.0, 20.0, 21.5, 7)
        on conflict (node_id, metric_name, day_bucket) do update set avg_value = excluded.avg_value;" >/dev/null
HIST=$(api_get "/api/metrics/history?node_id=$NODE_ID&start_time_ms=$D30&end_time_ms=$NOW_MS")
echo "$HIST" > "$QA_DIR/evidence/history.json"
check HISTORY-HOURLY-SOURCE 1 "$(echo "$HIST" | jq '[.[] | select(.source == "hourly")] | length | if . > 0 then 1 else 0 end')"
check HISTORY-DAILY-SOURCE 1 "$(echo "$HIST" | jq '[.[] | select(.source == "daily")] | length | if . > 0 then 1 else 0 end')"
check HISTORY-SORTED 1 "$(echo "$HIST" | jq '[.[].timestamp_ms] as $t | ($t == ($t | sort)) | if . then 1 else 0 end')"
check HISTORY-MISSING-PARAMS-400 400 "$(api_code GET /api/metrics/history)"

# --- README:358 "idle_gpus / avg_gpu_utilization / active_alerts are null ---
# when there is no data (never a fake 0)"
psql_q "delete from node_metrics;" >/dev/null
NO_DATA=$(curl -s "$HTTP/api/cluster/info")
echo "NOTE cluster/info with no metrics: $(echo "$NO_DATA" | jq -c '{idle_gpus, avg_gpu_utilization, active_alerts, running_jobs}')"
check CLUSTER-INFO-NULL-IDLE-AND-AVG 1 "$(echo "$NO_DATA" | jq 'if (.idle_gpus == null and .avg_gpu_utilization == null) then 1 else 0 end')"
check CLUSTER-INFO-NULL-ACTIVE-ALERTS 1 "$(echo "$NO_DATA" | jq 'if .active_alerts == null then 1 else 0 end')"

# --- dead config keys (declared in config.rs, never read anywhere else) -----
for k in redis_url prometheus_enabled prometheus_addr ws_heartbeat_interval_secs \
         ws_slow_threshold_ms ws_max_backlog max_concurrent_ws_clients tls_enabled \
         node_labels max_cached_reports reconnect_initial_delay_secs reconnect_max_delay_secs \
         disk_mounts log_level collect_process_details; do
  n=$(grep -rn "\b$k\b" "$REPO/crates" --include='*.rs' | grep -v 'crates/common/src/config.rs' | wc -l | tr -d ' ')
  echo "DEADKEY $k: usages_outside_config.rs=$n"
done
echo "NOTE dead keys above are recorded by qa/docs-consistency.qa.md (expectation: 0 usages)"

echo "----"
[ "$FAILED" -eq 0 ] && echo "OPS-CHECKS: ALL PASS" || echo "OPS-CHECKS: FAILURES PRESENT"
exit "$FAILED"
