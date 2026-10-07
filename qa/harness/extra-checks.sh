#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# QA stage-5 addition (no existing constraint changed, nothing skipped).
# Automates the checks that qa/security.qa.md + qa/deploy-ops.qa.md describe but
# the stage-1 scripts did not cover:
#   S12 -> SEC-13  login has no IP/global rate limit
#   S15 -> SEC-16  password_hash is not exposed by GET /api/users
#   S16 -> SEC-17  logs?limit= and list?page_size= are clamped
#   C15 -> CON-15  gRPC pending-jobs stream stops after the agent disconnects
#   O15 -> OPS-05  container tooling availability (compose path)
#   O19 -> OPS-09  GPU inventory (pure-CPU fallback coverage)
#   O20 -> OPS-10  listening ports + firewall CLI availability
#   --  -> FE-01   frontend absence on this line, presence in tree A
#   --  -> MRG-01  merge-plan material pointers are readable
# Prints "CHECK <id>: PASS|FAIL ..." and "NOTE ..." lines; exit 1 if any FAIL.
# Preconditions: server + agent down.  Leaves both stopped at the end.
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
note() { echo "NOTE $1"; }

mkdir -p "$REPO/qa/evidence"
echo "### host=$(hostname) commit=$(git -C "$REPO" rev-parse --short HEAD) at $(date -u +%Y-%m-%dT%H:%M:%SZ)"

sh "$HERE/agent-down.sh" >/dev/null 2>&1
sh "$HERE/server-down.sh" >/dev/null 2>&1

# ============ S12: login rate limiting (no such user, 20 attempts) ==========
sh "$HERE/server-up.sh" false >/dev/null
TOKEN=$(login "$ADMIN_USER" "$ADMIN_PASS")
[ -n "$TOKEN" ] || { echo "FATAL: cannot log in"; exit 1; }

CODES="$QA_DIR/login-codes.txt"
: > "$CODES"
i=1
while [ "$i" -le 20 ]; do
  curl -s -o /dev/null -w '%{http_code}\n' --max-time 5 -X POST \
    -H 'Content-Type: application/json' \
    -d '{"username":"no-such-user","password":"x"}' "$HTTP/api/login" >> "$CODES"
  i=$((i + 1))
done
note "S12 status histogram: $(sort "$CODES" | uniq -c | tr '\n' ' ')"
check SEC-13-NO-IP-RATE-LIMIT 0 "$(grep -c '^429$' "$CODES" | awk '{print ($1>0)?1:0}')"
check SEC-13-ALL-ATTEMPTS-401 20 "$(grep -c '^401$' "$CODES")"

# ============ S15: password_hash must not be exposed =======================
USERS=$(curl -s -H "Authorization: Bearer $TOKEN" --max-time 5 "$HTTP/api/users")
note "S15 GET /api/users first row: $(echo "$USERS" | jq -c '.[0] | {username, password_hash}')"
check SEC-16-PASSWORD-HASH-EMPTY 1 "$(echo "$USERS" | jq '[.[] | select(.password_hash != "")] | length | if . == 0 then 1 else 0 end')"

# ============ S16: input clamps (logs limit / list page_size) ==============
# GET /api/jobs answers with {"jobs":[…],"total":n}, not a bare array.
JOBS=$(curl -s -H "Authorization: Bearer $TOKEN" --max-time 5 "$HTTP/api/jobs")
note "S16 GET /api/jobs shape: $(echo "$JOBS" | jq -c '{type: type, keys: keys, total: .total}')"
JOB=$(echo "$JOBS" | jq -r '(.jobs // [])[0].job_id // empty')
note "S16 probe job_id=$JOB"
if [ -n "$JOB" ]; then
  N0=$(curl -s -H "Authorization: Bearer $TOKEN" --max-time 5 "$HTTP/api/jobs/$JOB/logs?limit=0" | jq 'length')
  N1=$(curl -s -H "Authorization: Bearer $TOKEN" --max-time 5 "$HTTP/api/jobs/$JOB/logs?limit=99999" | jq 'length')
  note "S16 logs limit=0 -> $N0 row(s); limit=99999 -> $N1 row(s) (clamped to 1..10000)"
  check SEC-17-LOGS-LIMIT-CLAMPED 1 "$(echo "$N0" | awk '{print ($1>=1)?1:0}')"
  check SEC-17-LOGS-LIMIT-UPPER 1 "$(echo "$N1" | awk '{print ($1<=10000)?1:0}')"
else
  note "S16 logs probe skipped: no job rows to probe with"
fi
for ps in 0 99999; do
  code=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $TOKEN" --max-time 5 "$HTTP/api/jobs?page_size=$ps")
  note "S16 list page_size=$ps -> HTTP $code"
  check "SEC-17-PAGE-SIZE-$ps-ACCEPTED" 200 "$code"
done

# ============ C15: pending-jobs stream exits when the agent goes away =====
# The loop-exit guard (grpc.rs:347-348) is only reachable when a send fails,
# which needs a job in `starting` for that node at the disconnect instant; the
# runtime part therefore records the absence of stream errors after the agent
# leaves, and the guard itself is verified by reading the source lines below.
sh "$HERE/agent-up.sh" qa-node-01 >/dev/null
sleep 8
ERRS0=$(grep -c 'get_jobs_for_node failed' "$QA_DIR/server.log")
note "C15 stream error lines with agent up: $ERRS0"
sh "$HERE/agent-down.sh" >/dev/null
sleep 20
ERRS1=$(grep -c 'get_jobs_for_node failed' "$QA_DIR/server.log")
note "C15 stream error lines 20s after agent-down: $ERRS1 — no growth means no orphaned polling task spamming the log"
check CON-15-STREAM-NO-ERROR-GROWTH 1 "$(echo "$ERRS0 $ERRS1" | awk '{print ($1==$2)?1:0}')"
note "C15 residual gap: the send-failure branch itself was verified by static reading only (not constructed at runtime)."
echo "--- grpc.rs loop-exit guard ---"
grep -n -A2 'if tx.send(Ok(job_to_proto(&job))).await.is_err()' "$REPO/crates/server/src/grpc.rs" || true

sh "$HERE/server-down.sh" >/dev/null
sh "$HERE/agent-down.sh" >/dev/null

# ============ C8 static side: retry columns are never written =============
echo "### C8 retry_count / max_retries usage outside the two literal zeros"
grep -rn 'retry_count\|max_retries' "$REPO/crates" --include='*.rs' | grep -v 'retry_count: 0\|max_retries: 0\|retry_count INTEGER\|max_retries INTEGER' || true

# ============ O15 / O19 / O20: environment facts ===========================
echo "### O15 container tooling"
command -v docker || echo "no-docker"
if command -v podman >/dev/null 2>&1; then
  echo "podman version: $(podman --version 2>&1)"
  echo "podman local image count: $(podman images --format '{{.Repository}}:{{.Tag}}' 2>/dev/null | wc -l)"
else
  echo "no-podman"
fi
echo "  => the compose path of deploy/docker-compose.yml cannot be exercised here (OPS-05 = N/A, static checks only)"
echo "### O19 GPU inventory"
note "O19 nvidia-smi GPU count: $(nvidia-smi --query-gpu=index --format=csv,noheader 2>/dev/null | wc -l)"
note "O19 'NVML init failed' lines in last agent log: $(grep -c 'NVML init failed' "$QA_DIR/agent.log" 2>/dev/null || echo 0)"
echo "  => README:323 pure-CPU fallback stays unverified: this host always has 6 L20s (OPS-09 = N/A)"
echo "### O20 listeners / firewall"
ss -ltn 2>/dev/null | grep -E ':8080|:50051|:9090' || echo "no 8080/50051/9090 listener right now (server stopped)"
if command -v firewall-cmd >/dev/null 2>&1; then
  echo "firewall-cmd present: $(command -v firewall-cmd)"
  echo "firewall-cmd --state: $(firewall-cmd --state 2>&1 | head -2)"
else
  echo "no-firewall-cli"
fi
echo "  => no root on this host; firewall policy itself stays unverified (OPS-10 = N/A)"

# ============ FE-01: frontend absence on this line =========================
echo "### FE-01 frontend inventory"
note "FE-01 gh-line web/ -> $(ls -d "$REPO/web" 2>&1)"
note "FE-01 gh-line package.json count: $(find "$REPO" -maxdepth 3 -name 'package.json' -not -path "$REPO/target/*" | wc -l)"
note "FE-01 gh-line html/ts/js count: $(find "$REPO" -maxdepth 4 \( -name '*.ts' -o -name '*.tsx' -o -name '*.html' \) -not -path "$REPO/target/*" -not -path "$REPO/gauntlet-out/*" | wc -l)"
note "FE-01 tree A local-wip/web files: $(find /public/tianyuyang/code/ClusterScope-review/local-wip/web -type f 2>/dev/null | wc -l)"

# ============ MRG-01: merge-plan material pointers =========================
echo "### MRG-01 material pointers"
ls -la /public/tianyuyang/code/ClusterScope-review/node-line.bundle /public/tianyuyang/code/ClusterScope-review/local-wip/ 2>&1
git -C /public/tianyuyang/code/ClusterScope status --short --branch 2>&1 | head -20
git -C /public/tianyuyang/code/ClusterScope log --oneline -3 2>&1
git -C /public/tianyuyang/code/ClusterScope rev-list --count HEAD 2>&1

echo "----"
[ "$FAILED" -eq 0 ] && echo "EXTRA-CHECKS: ALL PASS" || echo "EXTRA-CHECKS: FAILURES PRESENT"
exit "$FAILED"
