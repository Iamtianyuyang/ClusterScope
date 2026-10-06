#!/bin/sh
HERE=$(cd "$(dirname "$0")" && pwd)
# REST auth / authorization matrix for ClusterScope (read-only mode server).
# Preconditions: `sh qa/harness/server-up.sh false` (auth_required=false) is up.
# Prints one "CHECK <id>: PASS|FAIL detail" line per check; exit 1 if any FAIL.
# Evidence: append the whole output to $QA_DIR/evidence/api-checks.txt
. "$HERE/env.sh"

FAILED=0
check() { # check <id> <expected> <actual>
  if [ "$2" = "$3" ]; then
    echo "CHECK $1: PASS expected=$2 actual=$3"
  else
    echo "CHECK $1: FAIL expected=$2 actual=$3"
    FAILED=1
  fi
}
note() { echo "NOTE $1"; }

TOKEN=$(login "$ADMIN_USER" "$ADMIN_PASS")
[ -n "$TOKEN" ] && note "login ok (token len ${#TOKEN})" || note "login FAILED"

# --- public endpoints -------------------------------------------------------
check SEC-HEALTH-OPEN 200 "$(api_code GET /api/health)"
check SEC-NODES-READONLY-OPEN 200 "$(api_code GET /api/nodes)"
check SEC-METRICS-READONLY-OPEN 200 "$(api_code GET "/api/metrics/history?node_id=$NODE_ID&start_time_ms=0&end_time_ms=1")"

# --- writes without a token in read-only mode -------------------------------
check SEC-JOB-POST-NOTOKEN 401 "$(api_code POST /api/jobs '' '{"node_id":"'"$NODE_ID"'","name":"n","executable":"/bin/true","arguments":[],"working_directory":"/tmp"}')"
check SEC-USER-POST-NOTOKEN 401 "$(api_code POST /api/users '' '{"username":"nope","password":"nope123"}')"
check SEC-RULE-POST-NOTOKEN 401 "$(api_code POST /api/alerts/rules '' '{"name":"n","metric":"load_1","operator":"gt","threshold":1}')"

# --- GETs on admin-scoped routes (README:233 claims every GET is open) ------
check DOC-GET-USERS-READONLY 200 "$(api_code GET /api/users)"
check DOC-GET-AUDIT-LOGS 200 "$(api_code GET /api/audit-logs)"
check DOC-GET-PROMETHEUS 200 "$(api_code GET /api/prometheus/metrics)"

# --- forged / expired / valid JWTs -----------------------------------------
MINT="python3 $HERE/jwt-mint.py"
FORGED=$($MINT "wrong-secret-0123456789" admin 3600)
EXPIRED=$($MINT "qa-harness-secret-0123456789abcdef" admin -3600)
VALID_VIEWER=$($MINT "qa-harness-secret-0123456789abcdef" viewer 3600)
check SEC-JWT-FORGED 401 "$(api_code GET /api/jobs "$FORGED")"
check SEC-JWT-EXPIRED 401 "$(api_code GET /api/jobs "$EXPIRED")"
check SEC-JWT-VALID 200 "$(api_code GET /api/jobs "$VALID_VIEWER")"
check SEC-JWT-VIEWER-CANNOT-POST-JOB 403 "$(api_code POST /api/jobs "$VALID_VIEWER" '{"node_id":"'"$NODE_ID"'","name":"n","executable":"/bin/true","arguments":[],"working_directory":"/tmp"}')"
check SEC-GARBAGE-TOKEN 401 "$(api_code GET /api/jobs "not-a-jwt")"
check SEC-VIEWER-GET-USERS-403 403 "$(api_code GET /api/users "$VALID_VIEWER")"

# --- refresh-token rotation -------------------------------------------------
REFRESH=$(curl -s -X POST -H 'Content-Type: application/json' \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASS\"}" "$HTTP/api/login" | jq -r '.refresh_token // empty')
ROT1=$(curl -s -X POST -H 'Content-Type: application/json' -d "{\"refresh_token\":\"$REFRESH\"}" "$HTTP/api/refresh-token" | jq -r '.refresh_token // empty')
check SEC-REFRESH-ROTATION 401 "$(api_code POST /api/refresh-token '' "{\"refresh_token\":\"$REFRESH\"}")"
check SEC-REFRESH-NEW-TOKEN-OK 200 "$(api_code POST /api/refresh-token '' "{\"refresh_token\":\"$ROT1\"}")"

# --- login lockout (dedicated user, admin account is left untouched) --------
LOCKUSER="qa-lock-$(date +%s)"
curl -s -o /dev/null -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"username\":\"$LOCKUSER\",\"password\":\"lockme-123456\",\"role\":\"viewer\"}" "$HTTP/api/users"
i=1
while [ "$i" -le 5 ]; do
  api_code POST /api/login '' "{\"username\":\"$LOCKUSER\",\"password\":\"wrong-$i\"}" >/dev/null
  i=$((i + 1))
done
check SEC-LOGIN-LOCKOUT 429 "$(api_code POST /api/login '' "{\"username\":\"$LOCKUSER\",\"password\":\"lockme-123456\"}")"

# --- user input limits ------------------------------------------------------
check SEC-CREATE-USER-SHORT-PASSWORD 400 "$(api_code POST /api/users "$TOKEN" '{"username":"qa-shortpw","password":"12345"}')"
check SEC-CREATE-USER-BAD-ROLE 400 "$(api_code POST /api/users "$TOKEN" '{"username":"qa-badrole","password":"123456","role":"root"}')"

echo "----"
[ "$FAILED" -eq 0 ] && echo "API-CHECKS: ALL PASS" || echo "API-CHECKS: FAILURES PRESENT"
exit "$FAILED"
