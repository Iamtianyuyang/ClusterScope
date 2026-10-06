#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# Job lifecycle + alert + WebSocket end-to-end checks against a real server+agent.
# Preconditions:
#   sh qa/harness/server-up.sh false     (server on 127.0.0.1:8080)
#   sh qa/harness/agent-up.sh            (agent registered, node online)
# Prints "CHECK <id>: PASS|FAIL detail" lines; exit 1 if any FAIL.
# Evidence: $QA_DIR/evidence/job-e2e.txt (+ job logs copied next to it)
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

# wait_job <job_id> <status> <seconds>
wait_job() {
  i=0
  while [ "$i" -lt "$3" ]; do
    s=$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$1" | jq -r '.status // empty')
    [ "$s" = "$2" ] && return 0
    i=$((i + 1)); sleep 1
  done
  return 1
}

mkdir -p "$QA_DIR/evidence"

# --- 1. happy path: a short job reaches `succeeded` -------------------------
J1=$(curl -s -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"node_id":"'"$NODE_ID"'","name":"qa-echo","executable":"/bin/sh",
       "arguments":["-c","echo qa-hello; sleep 1; echo qa-done"],
       "working_directory":"/tmp"}' "$HTTP/api/jobs" | jq -r '.job_id // empty')
echo "NOTE job1=$J1"
wait_job "$J1" succeeded 90
check JOB-SUCCEEDED succeeded "$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$J1" | jq -r '.status')"
LOGS=$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$J1/logs")
echo "$LOGS" > "$QA_DIR/evidence/job1-logs.json"
check JOB-LOGS-CAPTURED 2 "$(echo "$LOGS" | jq '[.[] | select(.log_data | test("qa-hello|qa-done"))] | length')"

# --- 2. cancellation: SIGTERM reaches the process group ---------------------
J2=$(curl -s -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"node_id":"'"$NODE_ID"'","name":"qa-sleep","executable":"/bin/sh",
       "arguments":["-c","echo qa-sleep-start; sleep 300; echo qa-never"],
       "working_directory":"/tmp"}' "$HTTP/api/jobs" | jq -r '.job_id // empty')
echo "NOTE job2=$J2"
wait_job "$J2" running 90
check JOB-RUNNING running "$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$J2" | jq -r '.status')"
curl -s -o /dev/null -X DELETE -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$J2"
wait_job "$J2" cancelled 60
check JOB-CANCELLED cancelled "$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$J2" | jq -r '.status')"
# the SIGTERM must have hit the process group: no `sleep 300` may survive
sleep 2
check JOB-PROC-GONE 0 "$(pgrep -f 'sleep 300' | wc -l | tr -d ' ')"

# --- 3. terminal job cannot be stopped again -------------------------------
check JOB-STOP-TERMINAL-409 409 "$(api_code DELETE "/api/jobs/$J1" "$TOKEN")"
check JOB-STOP-UNKNOWN-404 404 "$(api_code DELETE "/api/jobs/does-not-exist" "$TOKEN")"

# --- 4. alert rule fires on real metrics -----------------------------------
RULE=$(curl -s -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"qa-load-rule","metric":"load_1","operator":"gte","threshold":0,
       "duration_seconds":0,"severity":"warning","node_id":"'"$NODE_ID"'"}' \
  "$HTTP/api/alerts/rules" | jq -r '.rule_id // empty')
echo "NOTE rule=$RULE"
i=0
while [ "$i" -lt 60 ]; do
  EV=$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/alerts/events" \
    | jq '[.[] | select(.rule_id == "'"$RULE"'")] | length')
  [ "$EV" != "0" ] && break
  i=$((i + 1)); sleep 1
done
check ALERT-EVENT-FIRED 1 "$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/alerts/events" \
  | jq '[.[] | select(.rule_id == "'"$RULE"'" and (.new_state == "pending" or .new_state == "firing"))] | length' | awk '{print ($1>0)?1:0}')"
check ALERT-CLUSTER-COUNT 1 "$(curl -s "$HTTP/api/cluster/info" | jq '.active_alerts')"
check ALERT-RULE-DELETE-CASCADE 200 "$(api_code DELETE "/api/alerts/rules/$RULE" "$TOKEN")"
check ALERT-RULE-DELETED 0 "$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/alerts/rules" | jq '[.[] | select(.rule_id=="'"$RULE"'")] | length')"

# --- 5. WebSocket still serves real-time updates ---------------------------
# A job is created while the socket is open so job_update pushes are observed too.
NODEBIN=$(command -v node || echo /public/tianyuyang/.nvm/versions/node/v26.1.0/bin/node)
"$NODEBIN" "$HERE/ws-check.mjs" "ws://127.0.0.1:8080/ws" "$NODE_ID" 25 \
  > "$QA_DIR/evidence/ws-check.txt" 2>&1 &
WSPID=$!
sleep 3
curl -s -o /dev/null -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"node_id":"'"$NODE_ID"'","name":"qa-ws","executable":"/bin/sh",
       "arguments":["-c","echo qa-ws"],"working_directory":"/tmp"}' "$HTTP/api/jobs"
wait "$WSPID"
WS=$(tail -1 "$QA_DIR/evidence/ws-check.txt")
echo "NOTE $WS"
check WS-CONNECTED 1 "$(echo "$WS" | sed -n 's/.*connected=\([01]\).*/\1/p')"
check WS-SUBSCRIBED 1 "$(echo "$WS" | sed -n 's/.*subscribed=\([01]\).*/\1/p')"
check WS-METRICS-PUSH 1 "$(echo "$WS" | sed -n 's/.*metrics=\([0-9]*\).*/\1/p' | awk '{print ($1>0)?1:0}')"
check WS-JOB-UPDATE-PUSH 1 "$(echo "$WS" | sed -n 's/.*jobs=\([0-9]*\).*/\1/p' | awk '{print ($1>0)?1:0}')"

echo "----"
[ "$FAILED" -eq 0 ] && echo "JOB-E2E: ALL PASS" || echo "JOB-E2E: FAILURES PRESENT"
exit "$FAILED"
