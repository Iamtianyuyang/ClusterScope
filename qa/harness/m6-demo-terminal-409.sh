#!/bin/sh
# M6 演示用（第 5 阶段 QA 产物）：终态任务再取消 = 409，未知名 = 404，queued 取消 = 200。
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/env.sh"

T=$(login "$ADMIN_USER" "$ADMIN_PASS")
J=$(psql_q "select job_id from jobs where status = 'succeeded' limit 1;")
if [ -n "$J" ]; then
  echo "已结束的任务 $J -> DELETE HTTP $(api_code DELETE "/api/jobs/$J" "$T" "")   （409 = 冲突，文档契约）"
else
  echo "库里暂时没有 succeeded 任务，跳过该条"
fi
echo "不存在的任务 -> DELETE HTTP $(api_code DELETE "/api/jobs/m6-demo-no-such-job" "$T" "")   （404）"
QJ=$(curl -s -X POST -H "Authorization: Bearer $T" -H 'Content-Type: application/json' \
  -d "{\"node_id\":\"$NODE_ID\",\"name\":\"m6-demo-queued\",\"executable\":\"/bin/true\",\"arguments\":[],\"working_directory\":\"/tmp\"}" \
  "$HTTP/api/jobs" | jq -r '.job_id // empty')
echo "新提交的 queued 任务 $QJ -> DELETE HTTP $(api_code DELETE "/api/jobs/$QJ" "$T" "")，状态变为 $(psql_q "select status from jobs where job_id='$QJ';")   （200 = 还没派发就撤销）"
