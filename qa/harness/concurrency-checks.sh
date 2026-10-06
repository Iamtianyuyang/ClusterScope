#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Concurrency / correctness checks: stale-starting requeue, GPU capacity FIFO,
# alert dedup, job pid persistence, stream-loop cleanup.
# Preconditions: server up (read-only mode) + agent up with node $NODE_ID online.
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
TOKEN=$(login "$ADMIN_USER" "$ADMIN_PASS")
[ -n "$TOKEN" ] || { echo "FATAL: cannot log in"; exit 1; }
mkdir -p "$QA_DIR/evidence"

job_status() { curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$1" | jq -r '.status // "missing"'; }
create_job() { # create_job <name> <quota> <shell-command>
  curl -s -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d '{"node_id":"'"$NODE_ID"'","name":"'"$1"'","executable":"/bin/sh",
         "arguments":["-c","'"$3"'"],"working_directory":"/tmp",
         "resource_quota":"'"$2"'"}' "$HTTP/api/jobs" | jq -r '.job_id // empty'
}

# --- 1. stale `starting` job on a node that never registered is requeued ----
# main.rs:404-435 (list_stale_starting_jobs + requeue_stale_job, node not online)
# jobs.node_id has an FK to node_info, so the ghost node needs a row there; it is
# never in the in-memory registry, which is what the requeue logic checks.
STALE="qa-stale-$(date +%s)"
psql_q "insert into node_info (node_id, hostname, ip_address) values ('qa-ghost-node', 'ghost', '127.0.0.1') on conflict (node_id) do nothing;" >/dev/null
psql_q "insert into jobs (job_id, node_id, name, executable, arguments, working_directory, environment, status, created_at, started_at, created_by, resource_quota)
        values ('$STALE', 'qa-ghost-node', 'stale', '/bin/true', '[]', '/tmp', '{}', 'starting', now() - interval '30 minutes', now() - interval '30 minutes', 'qa', 'gpu:999');" >/dev/null
i=0
while [ "$i" -lt 30 ]; do
  s=$(psql_q "select status from jobs where job_id='$STALE';")
  [ "$s" = "queued" ] && break
  i=$((i + 1)); sleep 1
done
check CON-REQUEUE-STALE-STARTING queued "$(psql_q "select status from jobs where job_id='$STALE';")"
check CON-REQUEUE-CLEARS-STARTED-AT t "$(psql_q "select started_at is null from jobs where job_id='$STALE';")"

# --- 2. GPU capacity is honoured: 6-GPU node runs one gpu:6 job at a time ---
J_A=$(create_job qa-cap-a gpu:6 "echo cap-a; sleep 25")
sleep 12
J_B=$(create_job qa-cap-b gpu:6 "echo cap-b; sleep 5")
sleep 12
check CON-CAPACITY-FIRST-RUNS running "$(job_status "$J_A")"
check CON-CAPACITY-SECOND-QUEUED queued "$(job_status "$J_B")"
# freeing capacity lets the queued job start
curl -s -o /dev/null -X DELETE -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$J_A"
i=0
while [ "$i" -lt 60 ]; do
  [ "$(job_status "$J_B")" = "running" ] && break
  [ "$(job_status "$J_B")" = "succeeded" ] && break
  i=$((i + 1)); sleep 1
done
echo "NOTE job B after cancelling A: $(job_status "$J_B")"
check CON-CAPACITY-FREED-DISPATCHES 1 "$(job_status "$J_B" | awk '{print ($1=="running"||$1=="succeeded")?1:0}')"

# --- 3. alert dedup: one firing event per (rule,node,gpu), no repeats ------
RULE=$(curl -s -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"qa-dedup","metric":"load_1","operator":"gte","threshold":0,"duration_seconds":0,
       "severity":"warning","node_id":"'"$NODE_ID"'"}' "$HTTP/api/alerts/rules" | jq -r '.rule_id // empty')
# Wait for the rule to reach `firing` (normal -> pending -> firing are separate
# events), then require that no further events appear while metrics keep coming.
i=0
while [ "$i" -lt 60 ]; do
  f=$(psql_q "select count(*) from alert_events where rule_id='$RULE' and new_state='firing';")
  [ "$f" != "0" ] && break
  i=$((i + 1)); sleep 1
done
sleep 3
C1=$(psql_q "select count(*) from alert_events where rule_id='$RULE';")
sleep 10
C2=$(psql_q "select count(*) from alert_events where rule_id='$RULE';")
echo "NOTE alert events for the rule: at firing=$C1, 10s later=$C2 (5 metrics reports in between)"
check CON-ALERT-DEDUP-NO-REPEAT 1 "$(echo "$C1 $C2" | awk '{print ($1 == $2) ? 1 : 0}')"
check CON-ALERT-DEDUP-SINGLE-TARGET 1 "$(echo "$C1" | awk '{print ($1 >= 1 && $1 <= 3) ? 1 : 0}')"
check CON-ALERT-FIRING-EVENT 1 "$(psql_q "select count(*) from alert_events where rule_id='$RULE' and new_state='firing';" | awk '{print ($1>0)?1:0}')"
curl -s -o /dev/null -X DELETE -H "Authorization: Bearer $TOKEN" "$HTTP/api/alerts/rules/$RULE"

# --- 4. jobs.pid is never persisted (update_job_status always passes None) --
check CON-JOB-PID-PERSISTED 1 "$(psql_q "select count(*) from jobs where pid is not null;" | awk '{print ($1>0)?1:0}')"
echo "NOTE jobs rows: $(psql_q "select count(*) from jobs;"), with pid: $(psql_q "select count(*) from jobs where pid is not null;")"
echo "NOTE agent log records real pids: $(grep -c 'Process spawned' "$QA_DIR/agent.log") spawn lines"

# --- 5. retry columns are never used by any code path ----------------------
check CON-RETRY-NEVER-USED 0 "$(psql_q "select count(*) from jobs where retry_count > 0 or max_retries > 0;" | awk '{print $1}')"

echo "----"
[ "$FAILED" -eq 0 ] && echo "CONCURRENCY-CHECKS: ALL PASS" || echo "CONCURRENCY-CHECKS: FAILURES PRESENT"
exit "$FAILED"
