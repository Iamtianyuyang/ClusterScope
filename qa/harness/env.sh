# ClusterScope QA harness — shared environment (POSIX sh, run on node 172.19.133.164).
# Source it:  . <repo>/qa/harness/env.sh
# Every path is absolute on purpose: the QA runner must not depend on cwd or PATH.
#
# 树定位（2026-10-07 M6 合流轮修正，理由见 qa/merge-m6.qa.md「harness 的树定位」）：
# REPO 此前**硬编码**成 gh-line 的绝对路径 —— 在任何别的工作树（如 .../merge-m6）里跑，
# 脚本会静默地测**旧树**（拿旧树的二进制与源码当证据），对合流验证是致命的。
# 现在按脚本自身位置解析（调用方都已定义 HERE），仍可用环境变量显式覆盖：
#   REPO=/public/tianyuyang/code/ClusterScope-review/gh-line sh qa/harness/api-checks.sh
if [ -z "${REPO:-}" ]; then
  if [ -n "${HERE:-}" ]; then
    REPO="$(cd "$HERE/../.." && pwd)"
  else
    echo "env.sh: 无法确定仓库根：HERE 与 REPO 都未定义（\$0=$0 不在 qa/harness 下）。" >&2
    echo "        用法：. <repo>/qa/harness/env.sh（由 qa/harness/*.sh 调用），或 REPO=<repo> sh qa/harness/<script>.sh" >&2
    return 2
  fi
fi
QA_DIR="$REPO/gauntlet-out/qa"          # gitignored scratch space (configs, logs, pids)
BIN="$REPO/target/release"              # release binaries built by `cargo build --release`
PG_BIN=/public/tianyuyang/code/ClusterScope-review/pg16/bin
PGDATA=/public/tianyuyang/code/ClusterScope-review/pgdata
PGLOG=/tmp/pg-server.log
PGURL="postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope"
HTTP="http://127.0.0.1:8080"
GRPC_ADDR="127.0.0.1:50051"
ADMIN_USER=admin
ADMIN_PASS=admin123
NODE_ID="${NODE_ID:-qa-node-01}"

# cargo is not on the default PATH on this host.
PATH="$HOME/.cargo/bin:$PATH"
export PATH

mkdir -p "$QA_DIR"

psql_q() { "$PG_BIN/psql" "$PGURL" -tAc "$1"; }
pg_is_up() { "$PG_BIN/pg_ctl" -D "$PGDATA" -l "$PGLOG" status >/dev/null 2>&1; }
pg_start() {
  pg_is_up || { "$PG_BIN/pg_ctl" -D "$PGDATA" -l "$PGLOG" start; sleep 2; }
}

# wait_http <url> <seconds> — 0 when the URL answers with any HTTP status.
wait_http() {
  url="$1"; secs="${2:-30}"; i=0
  while [ "$i" -lt "$secs" ]; do
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 "$url" 2>/dev/null)
    [ -n "$code" ] && [ "$code" != "000" ] && return 0
    i=$((i + 1)); sleep 1
  done
  return 1
}

# api_get <path> [token] — prints the response body.
api_get() {
  if [ -n "$2" ]; then
    curl -s -H "Authorization: Bearer $2" --max-time 5 "$HTTP$1"
  else
    curl -s --max-time 5 "$HTTP$1"
  fi
}

# api_code <method> <path> [token] [body] — prints only the HTTP status code.
api_code() {
  m="$1"; p="$2"; tok="$3"; body="$4"
  if [ -n "$body" ]; then
    if [ -n "$tok" ]; then
      curl -s -o /dev/null -w '%{http_code}' -X "$m" -H "Authorization: Bearer $tok" \
        -H 'Content-Type: application/json' -d "$body" --max-time 5 "$HTTP$p"
    else
      curl -s -o /dev/null -w '%{http_code}' -X "$m" \
        -H 'Content-Type: application/json' -d "$body" --max-time 5 "$HTTP$p"
    fi
  else
    if [ -n "$tok" ]; then
      curl -s -o /dev/null -w '%{http_code}' -X "$m" -H "Authorization: Bearer $tok" \
        --max-time 5 "$HTTP$p"
    else
      curl -s -o /dev/null -w '%{http_code}' -X "$m" --max-time 5 "$HTTP$p"
    fi
  fi
}

# login <user> <pass> — prints the access token (empty on failure).
login() {
  curl -s -X POST -H 'Content-Type: application/json' \
    -d "{\"username\":\"$1\",\"password\":\"$2\"}" --max-time 5 \
    "$HTTP/api/login" | jq -r '.access_token // empty'
}
