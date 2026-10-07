#!/bin/sh
# M6 演示用（第 5 阶段 QA 产物）：?force=true 与普通取消的区别。
# 前置：server + agent 已由 demo/*.json 的步骤起好；本脚本只提交/取消任务，不启停 server/agent。
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/env.sh"

T=$(login "$ADMIN_USER" "$ADMIN_PASS")
now_ms() { date +%s%3N; }
alive() { kill -0 "$1" 2>/dev/null && echo yes || echo no; }
submit() { # 提交一个忽略 SIGTERM 的任务
  curl -s -X POST -H "Authorization: Bearer $T" -H 'Content-Type: application/json' \
    -d "{\"node_id\":\"$NODE_ID\",\"name\":\"$1\",\"executable\":\"/bin/sh\",\"arguments\":[\"-c\",\"trap '' TERM; sleep 180\"],\"working_directory\":\"/tmp\"}" \
    "$HTTP/api/jobs" | jq -r '.job_id // empty'
}
waitpid() { # 等 agent 写下 pid 标记，回显进程组 leader 的 pid
  i=0
  while [ "$i" -lt 80 ]; do
    f="$QA_DIR/agent-logs/$1/pid"
    if [ -f "$f" ]; then head -1 "$f"; return 0; fi
    i=$((i + 1)); sleep 0.5
  done
  echo ""
}
waitgone() { # 轮询进程组消失，回显耗时毫秒
  t0=$(now_ms)
  i=0
  while [ "$i" -lt 600 ]; do
    [ "$(alive "$1")" = "no" ] && { echo "$(( $(now_ms) - t0 ))"; return 0; }
    i=$((i + 1)); sleep 0.1
  done
  echo "TIMEOUT"
}

J1=$(submit "m6-demo-term")
P1=$(waitpid "$J1")
echo "① 普通取消：任务 $J1 在跑（进程组 leader pid=$P1，cmdline: $(tr '\0' ' ' < "/proc/$P1/cmdline" 2>/dev/null)）"
echo "   该进程 trap '' TERM：SIGTERM 拿它没办法，只能靠 5s 后的 SIGKILL 升级。"
T0=$(now_ms)
echo "   DELETE /api/jobs/$J1 -> HTTP $(api_code DELETE "/api/jobs/$J1" "$T" "")"
G1=$(waitgone "$P1")
echo "   进程组消失耗时 = $G1 ms（含 agent 收到 stopping 的延迟 + 5s 宽限期）"
echo "   agent 日志：$(grep -c 'SIGTERM ignored, escalating to SIGKILL' "$QA_DIR/agent.log") 条 SIGKILL 升级记录"

J2=$(submit "m6-demo-force")
P2=$(waitpid "$J2")
echo "② 强制取消：任务 $J2 在跑（pid=$P2）"
T0=$(now_ms)
echo "   DELETE /api/jobs/$J2?force=true -> HTTP $(api_code DELETE "/api/jobs/$J2?force=true" "$T" "")"
G2=$(waitgone "$P2")
echo "   进程组消失耗时 = $G2 ms（跳过 5s 宽限期，agent 一看到 stopping 就 SIGKILL）"
echo "   agent 日志：$(grep -c 'Forced cancellation' "$QA_DIR/agent.log") 条强制取消记录"
echo "   终态：$(curl -s -H "Authorization: Bearer $T" "$HTTP/api/jobs/$J2" | jq -r '.status')"
