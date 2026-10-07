#!/bin/sh
# M6 演示用（第 5 阶段 QA 返工复验产物）：
#   ① F-23：锁定账号应答 429（合流时一度回归成 401，返工后在 3230e8b 恢复）
#   ② F-24：cluster/info 有数据时是真实数字、无数据时 idle_gpus / avg_gpu_utilization 是 JSON null
# 前置：server 已由 demo/*.json 的步骤起好（只读模式，auth_required:false）。
# 本脚本自己按 PID 文件起停 agent（agent-up.sh / agent-down.sh），结束时保持 agent 停止。
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/env.sh"

T=$(login "$ADMIN_USER" "$ADMIN_PASS")
echo "admin 登录: token len=${#T}"

echo "① F-23 锁定账号的状态码（期望 429；401 = 回归）"
LU="m6-demo-lock-$$"
echo "   建一个专用账号 $LU -> HTTP $(api_code POST /api/users "$T" "{\"username\":\"$LU\",\"password\":\"lockme-123456\",\"role\":\"viewer\"}")"
i=1
while [ "$i" -le 5 ]; do
  echo "   第 $i 次错误口令 -> HTTP $(api_code POST /api/login '' "{\"username\":\"$LU\",\"password\":\"wrong-$i\"}")"
  i=$((i + 1))
done
echo "   锁定期内用【正确】口令 -> HTTP $(api_code POST /api/login '' "{\"username\":\"$LU\",\"password\":\"lockme-123456\"}")   （期望 429）"
echo "   DB 行: $(psql_q "select 'failed=' || failed_login_attempts || ' locked_until=' || locked_until from users where username='$LU';")"

echo "② F-24 cluster/info 的两种状态（README:442 —— 没有数据时是 null，绝不是假的 0）"
sh "$HERE/agent-up.sh" qa-node-01 >/dev/null 2>&1
i=0
while [ "$i" -lt 30 ]; do
  n=$(psql_q "select count(*) from node_metrics where gpu_metrics is not null and node_id='$NODE_ID';")
  [ "${n:-0}" -gt 0 ] && break
  i=$((i + 1)); sleep 1
done
echo "   节点表（/api/nodes）: $(curl -s "$HTTP/api/nodes" | jq -c 'map({node_id, gpu_count, status})')"
echo "   有指标数据（$NODE_ID 的 gpu_metrics 非空行数 $(psql_q "select count(*) from node_metrics where gpu_metrics is not null and node_id='$NODE_ID';")）："
echo "   $(curl -s "$HTTP/api/cluster/info" | jq -c '{total_gpus, idle_gpus, avg_gpu_utilization, active_alerts}')"
sh "$HERE/agent-down.sh" >/dev/null 2>&1
psql_q "delete from node_metrics;" >/dev/null
echo "   无指标数据（node_metrics 行数 $(psql_q "select count(*) from node_metrics;")）："
echo "   $(curl -s "$HTTP/api/cluster/info" | jq -c '{total_gpus, idle_gpus, avg_gpu_utilization, active_alerts}')"
