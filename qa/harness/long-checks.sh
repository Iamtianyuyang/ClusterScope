#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# QA stage-5 addition (no existing constraint changed, nothing skipped).
# Executes the two `long` constraints from qa/constraints.json end to end on a
# real server + agent, plus a precise re-registration measurement for DOC-15:
#   CON-10  retention cadence: raw 24h (covered by ops-checks), hourly 7d,
#           daily 90d and job logs 30d — measured against the real 10s tick.
#   CON-11  hourly rollup appears at cycle 60 (600s) and a SECOND tick at
#           cycle 120 (1200s) must not duplicate the bucket.
#   DOC-15  after a server restart the agent re-registers within 60s.
# The 90d daily cleanup hangs off cycle % 360 (3600s, main.rs:368), so this
# script waits ~63 minutes to observe it; phases are printed with timestamps.
# Preconditions: server + agent down.  Leaves both stopped at the end.
# Evidence: qa/evidence/long-checks.txt  (run with nohup/setsid so a dropped
# ssh session does not kill the measurement).
. "$HERE/env.sh"

mkdir -p "$REPO/qa/evidence"
say() { echo "[$(date -u +%H:%M:%S)] $*"; }
check() {
  if [ "$2" = "$3" ]; then
    echo "CHECK $1: PASS expected=$2 actual=$3"
  else
    echo "CHECK $1: FAIL expected=$2 actual=$3"
  fi
}

sh "$HERE/agent-down.sh" >/dev/null 2>&1
sh "$HERE/server-down.sh" >/dev/null 2>&1

say "start: host=$(hostname) commit=$(git -C "$REPO" rev-parse --short HEAD)"

# --- DOC-15: measure how long the agent takes to come back after a restart --
sh "$HERE/server-up.sh" false >/dev/null
sh "$HERE/agent-up.sh" qa-node-01 >/dev/null
sleep 5
say "agent up; restarting the server to time the re-registration"
sh "$HERE/server-down.sh" >/dev/null
T0=$(date +%s)
sh "$HERE/server-up.sh" false >/dev/null
T1=$(date +%s)
i=0
while [ "$i" -lt 120 ]; do
  n=$(curl -s "$HTTP/api/nodes" | jq '[.[] | select(.node_id == "'"$NODE_ID"'")] | length')
  [ "$n" != "0" ] && break
  i=$((i + 1)); sleep 1
done
T2=$(date +%s)
say "DOC-15 server restart: health-ready after $((T1 - T0))s, $NODE_ID back $((T2 - T1))s after health (total $((T2 - T0))s)"
check DOC-15-NODE-REAPPEARS-LE75 1 "$(echo "$((T2 - T0))" | awk '{print ($1<=75)?1:0}')"

# --- fixtures for CON-10 / CON-11 ------------------------------------------
NOW_MS=$(python3 -c 'import time;print(int(time.time()*1000))')
JOB=$(psql_q "select job_id from jobs order by created_at desc limit 1;")
say "fixtures: job_id=$JOB"
psql_q "insert into metrics_hourly (node_id, metric_name, hour_bucket, avg_value, max_value, min_value, p95_value, sample_count)
        values ('$NODE_ID','qa-old-hourly', now() - interval '8 days', 1.0, 1.0, 1.0, 1.0, 1)
        on conflict (node_id, metric_name, hour_bucket) do nothing;" >/dev/null
psql_q "insert into metrics_daily (node_id, metric_name, day_bucket, avg_value, max_value, min_value, p95_value, sample_count)
        values ('$NODE_ID','qa-old-daily', (now() - interval '91 days')::date, 1.0, 1.0, 1.0, 1.0, 1)
        on conflict (node_id, metric_name, day_bucket) do nothing;" >/dev/null
[ -n "$JOB" ] && psql_q "insert into job_logs (job_id, log_offset, log_data, timestamp)
        values ('$JOB', 900001, 'qa-old-log', now() - interval '31 days')
        on conflict (job_id, log_offset) do nothing;" >/dev/null
say "fixtures inserted: hourly 8d=$(psql_q "select count(*) from metrics_hourly where metric_name='qa-old-hourly';"), daily 91d=$(psql_q "select count(*) from metrics_daily where metric_name='qa-old-daily';"), joblog 31d=$(psql_q "select count(*) from job_logs where log_offset=900001;")"

# --- tick 1 (cycle 60 => 600s) ---------------------------------------------
# cpu_usage_percent is the one metric every raw report carries, so it is the
# stable key for the rollup assertions.
sleep 640
B1=$(psql_q "select max(hour_bucket)::text from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent';")
ROWS1=$(psql_q "select count(*) from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent';")
SAMPLES1=$(psql_q "select sample_count from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent' and hour_bucket = '$B1'::timestamptz;")
say "tick1 done: newest cpu bucket=$B1, cpu rows for node=$ROWS1, sample_count=$SAMPLES1"
say "tick1 retention: hourly8d=$(psql_q "select count(*) from metrics_hourly where metric_name='qa-old-hourly';"), joblog31d=$(psql_q "select count(*) from job_logs where log_offset=900001;"), daily91d=$(psql_q "select count(*) from metrics_daily where metric_name='qa-old-daily';")"
check CON-10-HOURLY-7D-PRUNED 0 "$(psql_q "select count(*) from metrics_hourly where metric_name='qa-old-hourly';")"
check CON-10-JOBLOG-30D-PRUNED 0 "$(psql_q "select count(*) from job_logs where log_offset=900001;")"
check CON-11-HOURLY-BUCKET-CREATED 1 "$(echo "$ROWS1" | awk '{print ($1>=1)?1:0}')"
check CON-11-ONE-ROW-PER-BUCKET 1 "$(psql_q "select count(*) from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent' and hour_bucket = '$B1'::timestamptz;" | awk '{print ($1==1)?1:0}')"

# --- tick 2 (cycle 120 => 1200s) must re-upsert, not duplicate -------------
sleep 640
ROWS2=$(psql_q "select count(*) from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent';")
SAMPLES2=$(psql_q "select sample_count from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent' and hour_bucket = '$B1'::timestamptz;")
B2=$(psql_q "select max(hour_bucket)::text from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent';")
say "tick2 done: newest cpu bucket=$B2, cpu rows for node=$ROWS2, sample_count for $B1=$SAMPLES2 (tick1: $ROWS1 / $SAMPLES1)"
check CON-11-SECOND-TICK-NO-DUPLICATE 1 "$(echo "$ROWS1 $ROWS2" | awk '{print ($1==$2)?1:0}')"
check CON-11-BUCKET-STILL-SINGLE-ROW 1 "$(psql_q "select count(*) from metrics_hourly where node_id='$NODE_ID' and metric_name='cpu_usage_percent' and hour_bucket = '$B1'::timestamptz;" | awk '{print ($1==1)?1:0}')"

# --- tick 6 (cycle 360 => 3600s) runs the 90d daily cleanup and rollup -----
WAITED=$((640 + 640))
REMAIN=$((3720 - WAITED))
say "waiting ${REMAIN}s more for the cycle-360 tick (daily rollup + 90d cleanup)"
sleep "$REMAIN"
say "tick6 done: daily91d=$(psql_q "select count(*) from metrics_daily where metric_name='qa-old-daily';"), daily-rows-for-node=$(psql_q "select count(*) from metrics_daily where node_id='$NODE_ID';")"
check CON-10-DAILY-90D-PRUNED 0 "$(psql_q "select count(*) from metrics_daily where metric_name='qa-old-daily';")"

# --- cleanup ---------------------------------------------------------------
sh "$HERE/agent-down.sh" >/dev/null 2>&1
sh "$HERE/server-down.sh" >/dev/null 2>&1
say "done; server + agent stopped"
