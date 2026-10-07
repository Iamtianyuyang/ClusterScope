#!/bin/sh
# M6 演示用（第 5 阶段 QA 产物）：审计端点的筛选一致性与 total 口径。
# 前置：server 已由 demo/*.json 的 server-up 步骤起好；本脚本只读，不启停任何进程。
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/env.sh"

T=$(login "$ADMIN_USER" "$ADMIN_PASS")
echo "admin 登录: token len=${#T}"
for q in "" "&user=admin" "&action=login" "&start_time_ms=0" "&user=admin&action=login&start_time_ms=0"; do
  body=$(curl -s --max-time 5 -H "Authorization: Bearer $T" "$HTTP/api/audit-logs?page_size=500$q")
  echo "GET /api/audit-logs?page_size=500$q"
  echo "    -> $(printf '%s' "$body" | jq -c '{total, rows:(.logs|length)}')"
done
echo "psql 对照: select count(*) from audit_logs; -> $(psql_q 'select count(*) from audit_logs;')"
echo "坏参数 start_time_ms=abc -> HTTP $(api_code GET '/api/audit-logs?start_time_ms=abc' "$T" "")  （400 = 客户端错误，不再是 500）"
echo "不存在的 user -> $(api_get '/api/audit-logs?user=qa-absent-user' "$T" | jq -c '{total, rows:(.logs|length)}')"
echo "分页 page=1&page_size=5 -> $(api_get '/api/audit-logs?page=1&page_size=5' "$T" | jq -c '{total, rows:(.logs|length)}')"
