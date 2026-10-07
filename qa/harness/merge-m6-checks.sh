#!/bin/sh
# qa/harness/merge-m6-checks.sh
#
# M6 合流轮的判据程序 —— report/merge-plan.md 的「M9 合流后怎么证明没丢东西」+「M10 不变量 NRM1–NRM8」
# 的可执行版本。它**不重写**既有 harness 的判定：能复用的段直接调用既有脚本，只对合流新增的面另写检查
# （F-01 端点翻转、审计覆盖、登录限速的正判据、迁移与表清单、M6 轮的范围守卫、NRM 的 grep 族）。
#
# 用法：
#   sh qa/harness/merge-m6-checks.sh              # 全量（自己起停 server/agent，只按 PID 文件停）
#   sh qa/harness/merge-m6-checks.sh --static     # 只跑不需要 server/agent 的段（快，约 2 分钟）
#
# 输出：每行 "<ID> PASS|FAIL - 说明"；退出码 = FAIL 条数。
# 证据：gauntlet-out/qa/evidence/merge-m6-*.txt
# 硬规矩（qa/README.md）：这台机器是共享的 —— 只按 PID 文件停自己起的进程；不用按名字整机匹配的
# pkill/killall；不动本机既有的常驻 agent（systemd --user 的 clusterscope-agent.service）。
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/env.sh" || exit 2

EVID="$QA_DIR/evidence"
mkdir -p "$EVID"
LOG="$EVID/merge-m6-checks.txt"
: > "$LOG"

# 合流基线：审查产物 + 无 root 修复都已进主线的那个提交（M6 的起点）。
M6BASE="${M6_BASE:-8601ac9}"
# 合流前的实测基线（2026-10-07 实测于本工作树，逐条记录见 qa/merge-m6.qa.md §1）：
BASE_TESTS=59            # cargo test 通过数（F10 证据：rc=0 passed=59 failed=0）
BASE_API_PASS=18         # api-checks.sh 的 PASS 条数（20 条检查，2 条 FAIL）
BASE_DOC_FAIL=9          # doc-claims-checks.sh 的 FAIL 条数
BASE_NRM_PATHS=3         # 非注释、非测试的系统路径默认值条数（NRM3 的实质命中）
BASE_NRM_HITS=7          # 同一 grep 的全部命中行（含注释与测试夹具）
BASE_NRM5_HITS=2         # `setuid|setgid|pre_exec|pkexec|sudo |chown|setsid` 的命中（= pre_exec + setsid 一处）

STATIC_ONLY=0
[ "${1:-}" = "--static" ] && STATIC_ONLY=1

PASS=0
FAIL=0

verdict() { # verdict <ID> <0|1> <说明>
  if [ "$2" -eq 0 ]; then
    PASS=$((PASS + 1)); printf '%s PASS - %s\n' "$1" "$3" | tee -a "$LOG"
  else
    FAIL=$((FAIL + 1)); printf '%s FAIL - %s\n' "$1" "$3" | tee -a "$LOG"
  fi
}
note() { printf 'NOTE %s\n' "$1" | tee -a "$LOG"; }

echo "== 环境 ==" | tee -a "$LOG"
echo "repo=$REPO  branch=$(git -C "$REPO" branch --show-current)  head=$(git -C "$REPO" rev-parse --short HEAD)  m6base=$M6BASE  uid=$(id -u)" | tee -a "$LOG"
cd "$REPO" || exit 2

# ------------------------------------------------------------------ M6-01 构建
b_log="$EVID/merge-m6-build.txt"
( export PATH="$HOME/.cargo/bin:$PATH"; cargo build --workspace --all-targets --offline ) > "$b_log" 2>&1
rc=$?
verdict M6-01 "$rc" "cargo build --workspace --all-targets --offline 退出码 $rc（离线，不得引入新依赖）"

# ------------------------------------------------------------------ M6-02/03 测试
t_log="$EVID/merge-m6-tests.txt"
( export PATH="$HOME/.cargo/bin:$PATH"; cargo test --workspace --offline ) > "$t_log" 2>&1
rc=$?
passed=$(grep -oE '[0-9]+ passed' "$t_log" | awk '{s+=$1} END{print s+0}')
failed=$(grep -oE '[0-9]+ failed' "$t_log" | awk '{s+=$1} END{print s+0}')
deleted=$(git diff "$M6BASE" -- crates tests 2>/dev/null | grep -E '^-[^-].*#\[(tokio::)?test\]' || true)
{
  echo "rc=$rc passed=$passed failed=$failed"
  grep -E '^test result|^error' "$t_log" || true
  echo "== 相对 $M6BASE 被删掉的 #[test] 行 =="; echo "${deleted:-<none>}"
} >> "$LOG"
verdict M6-02 "$([ "$rc" = 0 ] && [ "$failed" = 0 ] && echo 0 || echo 1)" \
  "cargo test：rc=$rc、failed=$failed、passed=$passed（要求 0 失败）"
verdict M6-03 "$([ "$passed" -ge "$BASE_TESTS" ] && [ -z "$deleted" ] && echo 0 || echo 1)" \
  "通过数 $passed ≥ 基线 $BASE_TESTS，且没有删除既有 #[test] 行"

# ------------------------------------------------------------------ M6-04 范围守卫（与 F12 互为对照的独立实现）
allow() { # allow <路径> → 0 = 在 M6 允许集内
  case "$1" in
    deploy/nginx.conf) return 1 ;;                       # M5：web 面本轮不做
    crates/*|tests/*|Cargo.toml|Cargo.lock) return 0 ;;
    deploy/*|README.md|docs/*|features/*|qa/*|demo/*|report/*|GAUNTLET.md|gauntlet-tools/*) return 0 ;;
    *) return 1 ;;
  esac
}
changed=$( { git diff --name-only "$M6BASE"; git ls-files --others --exclude-standard; } | sort -u)
bad=""
for f in $changed; do allow "$f" || bad="$bad $f"; done
frozen=$(git diff --name-only "$M6BASE" -- web deploy/nginx.conf .gauntlet gauntlet.config.json gauntlet-baseline.json 2>/dev/null || true)
{
  echo "== 改动/新增的文件（相对 $M6BASE）=="; echo "$changed"
  echo "== 允许集之外的 =="; echo "${bad:-<none>}"
  echo "== 冻结面（web/、deploy/nginx.conf、.gauntlet/、gauntlet.config.json、gauntlet-baseline.json）=="; echo "${frozen:-<none>}"
} >> "$LOG"
verdict M6-04 "$([ -z "$bad" ] && [ -z "$frozen" ] && echo 0 || echo 1)" \
  "改动全部落在 M6 允许集内、冻结面零改动（越界：${bad:-无}；冻结面：${frozen:-无}）"

# ------------------------------------------------------------------ M6-05/06 NRM3 / NRM5（不变量）
nrm_log="$EVID/merge-m6-nrm.txt"
NRM3_CMD="grep -rn '/etc/clusterscope\\|/var/lib/clusterscope\\|/var/log/clusterscope\\|/usr/local/bin' crates deploy/install-agent.sh deploy/tui.sh"
{
  echo "== NRM3 全部命中（含注释与测试夹具；基线 $BASE_NRM_HITS 行）=="
  eval "$NRM3_CMD"
  echo "== NRM3 实质命中（去掉测试夹具与整行注释；基线 $BASE_NRM_PATHS 行，每一处都必须是可覆盖的默认值）=="
  eval "$NRM3_CMD" | grep -v 'crates/agent/tests/' | grep -vE ':[0-9]+: *(//|#)'
  echo "== NRM5 特权原语（setuid/setgid/pre_exec/pkexec/sudo/chown/setsid；基线 $BASE_NRM5_HITS 行）=="
  grep -rnE 'setuid|setgid|pre_exec|pkexec|sudo |chown|setsid' crates 2>/dev/null || true
  echo "== NRM5 的 pre_exec 上下文（只允许进程组设置，紧跟 setsid）=="
  grep -rn -A1 'pre_exec' crates 2>/dev/null || true
  echo "== NRM5 真正要排除的提权调用（必须 0 行）=="
  grep -rnE 'setuid|setgid|pkexec|chown|sudo ' crates 2>/dev/null || true
} >> "$nrm_log"
nrm_hits=$(eval "$NRM3_CMD" | wc -l | tr -d ' ')
nrm_paths=$(eval "$NRM3_CMD" | grep -v 'crates/agent/tests/' | grep -vE ':[0-9]+: *(//|#)' | wc -l | tr -d ' ')
priv=$(grep -rnE 'setuid|setgid|pkexec|chown|sudo ' crates 2>/dev/null | wc -l | tr -d ' ')
nrm5=$(grep -rnE 'setuid|setgid|pre_exec|pkexec|sudo |chown|setsid' crates 2>/dev/null | wc -l | tr -d ' ')
setsid_next=$(grep -rn -A1 'pre_exec' crates 2>/dev/null | grep -c 'setsid' | tr -d ' ')
verdict M6-05 "$([ "$nrm_paths" -le "$BASE_NRM_PATHS" ] && [ "$nrm_hits" -le "$BASE_NRM_HITS" ] && echo 0 || echo 1)" \
  "NRM3：系统路径默认值 $nrm_paths 处 ≤ 基线 $BASE_NRM_PATHS（全部命中 $nrm_hits ≤ $BASE_NRM_HITS）"
verdict M6-06 "$([ "$priv" = 0 ] && [ "$nrm5" -le "$BASE_NRM5_HITS" ] && [ "$setsid_next" -ge 1 ] && echo 0 || echo 1)" \
  "NRM5：提权原语 $priv 处（必须 0）；特权/进程组族命中 $nrm5 ≤ 基线 $BASE_NRM5_HITS（其中 pre_exec 旁有 setsid $setsid_next 处）"

# ------------------------------------------------------------------ M6-07 无 root 四项修复不回归
# 默认用 --no-slow：F1 的动态探针与 F5 的 systemd「真装真启」在这台**共享机器**上会临时换成
# 操作者自己的 ~/.config/systemd/user/clusterscope-agent.service（既有 finding F-22：脚本会逐字节还原、
# MainPID 不变，但本轮任务书明确要求「不要碰常驻 agent PID 266643 及其 unit/配置」）。
# 需要跑全量时显式打开：M6_FULL_NOROOT=1 sh qa/harness/merge-m6-checks.sh
nrf_log="$EVID/merge-m6-no-root-fixes.txt"
NRF_ARGS="--no-slow"
[ "${M6_FULL_NOROOT:-0}" = 1 ] && NRF_ARGS=""
note "M6-07 用 no-root-fixes-checks.sh $NRF_ARGS（F1 动态探针与 F5 真装真启默认跳过，理由见脚本注释/qa/merge-m6.qa.md）"
( export PATH="$HOME/.cargo/bin:$PATH"; sh "$HERE/no-root-fixes-checks.sh" $NRF_ARGS ) > "$nrf_log" 2>&1
nrf_pass=$(grep -cE '^F[0-9]+ PASS' "$nrf_log")
nrf_fail=$(grep -cE '^F[0-9]+ FAIL' "$nrf_log")
verdict M6-07 "$([ "$nrf_pass" = 12 ] && [ "$nrf_fail" = 0 ] && echo 0 || echo 1)" \
  "no-root-fixes-checks.sh（$NRF_ARGS）：PASS=$nrf_pass FAIL=$nrf_fail（要求 12/0）"

# ------------------------------------------------------------------ M6-08/09/10 文档一致性：FAIL 不增加 + 两个翻转 + TUI 键 + 配置键
doc_log="$EVID/merge-m6-doc-claims.txt"
sh "$HERE/doc-claims-checks.sh" > "$doc_log" 2>&1
doc_fail=$(grep -c ': FAIL' "$doc_log")
tui_keys=$(grep -c '^CHECK DOC-TUI-KEY .*: PASS' "$doc_log")
yaml_keys=$(grep -cE '^CHECK DOC-(SERVER|AGENT)-YAML-KEY .*: PASS' "$doc_log")
sigkill=$(grep -c '^CHECK DOC-CODE-SIGKILL-EXISTS: PASS' "$doc_log")
force=$(grep -c '^CHECK DOC-CODE-FORCE-OPTION: PASS' "$doc_log")
verdict M6-08 "$([ "${doc_fail:-99}" -le "$BASE_DOC_FAIL" ] && [ "$sigkill" = 1 ] && [ "$force" = 1 ] && echo 0 || echo 1)" \
  "doc-claims：FAIL=$doc_fail ≤ 基线 $BASE_DOC_FAIL；SIGKILL 断言 PASS=$sigkill、force 断言 PASS=$force（合流后必须由 FAIL 转 PASS）"
verdict M6-09 "$([ "$tui_keys" = 13 ] && echo 0 || echo 1)" \
  "TUI 快捷键 13/13 PASS（实际 $tui_keys）"
verdict M6-10 "$([ "$yaml_keys" = 17 ] && echo 0 || echo 1)" \
  "配置键 17/17 PASS（实际 $yaml_keys）"

# ================================================================== 运行时段
if [ "$STATIC_ONLY" = 1 ]; then
  note "跳过运行时段（--static）：M6-11…M6-15 未执行"
else
  startsrv() { sh "$HERE/server-up.sh" false > "$QA_DIR/server-up-m6.log" 2>&1; }
  stopsrv() { sh "$HERE/server-down.sh" >> "$QA_DIR/server-up-m6.log" 2>&1; }
  startsrv
  health=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$HTTP/api/health")
  if [ "$health" != 200 ]; then
    note "server 起不来（/api/health=$health）—— 运行时段全部记 FAIL，原始输出见 $QA_DIR/server-up-m6.log"
  fi

  # ---------------------------------------------------------------- M6-11 REST 矩阵 + F-01 翻转
  api_log="$EVID/merge-m6-api-checks.txt"
  sh "$HERE/api-checks.sh" > "$api_log" 2>&1
  api_pass=$(grep -c ': PASS' "$api_log")
  audit_flip=$(grep -c '^CHECK DOC-GET-AUDIT-LOGS: PASS' "$api_log")
  verdict M6-11 "$([ "$api_pass" -ge "$BASE_API_PASS" ] && [ "$audit_flip" = 1 ] && echo 0 || echo 1)" \
    "api-checks：PASS=$api_pass（要求 ≥ 基线 $BASE_API_PASS）；DOC-GET-AUDIT-LOGS 已 PASS=$audit_flip（要求 1 = F-01 修好了）"

  # ---------------------------------------------------------------- M6-12 审计覆盖（F-10）
  TOKEN=$(login "$ADMIN_USER" "$ADMIN_PASS")
  T0=$(psql_q "select now();")
  curl -s -o /dev/null -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d "{\"username\":\"m6-audit-$(date +%s)\",\"password\":\"m6-audit-123456\",\"role\":\"viewer\"}" "$HTTP/api/users"
  curl -s -o /dev/null -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d '{"node_id":"'"$NODE_ID"'","name":"m6-audit","executable":"/bin/true","arguments":[],"working_directory":"/tmp"}' "$HTTP/api/jobs"
  J=$(curl -s -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs" | jq -r '.jobs[0].job_id // empty' 2>/dev/null)
  [ -n "$J" ] && curl -s -o /dev/null -X DELETE -H "Authorization: Bearer $TOKEN" "$HTTP/api/jobs/$J"
  curl -s -o /dev/null -X POST -H 'Content-Type: application/json' \
    -d '{"username":"m6-nobody","password":"wrong-m6"}' "$HTTP/api/login"
  sleep 2
  actions=$(psql_q "select count(distinct action) from audit_logs where timestamp > '$T0';")
  rows=$(psql_q "select count(*) from audit_logs where timestamp > '$T0';")
  list_total=$(api_get "/api/audit-logs" "$TOKEN" | jq -r '.total // empty' 2>/dev/null)
  {
    echo "audit rows since $T0: $rows, distinct actions: $actions, GET /api/audit-logs total: ${list_total:-<none>}"
    echo "action histogram:"; psql_q "select action, count(*) from audit_logs where timestamp > '$T0' group by action order by 2 desc;"
  } >> "$LOG"
  verdict M6-12 "$([ "${actions:-0}" -ge 3 ] && [ -n "${list_total:-}" ] && echo 0 || echo 1)" \
    "审计覆盖：新动作种类 ${actions:-0}（要求 ≥3，合流前是 2）；GET /api/audit-logs 的 total=${list_total:-<读不到>}（要求非空 = 端点可用）"

  # ---------------------------------------------------------------- M6-13 登录限速的正判据（F-08）
  codes="$QA_DIR/merge-m6-login-codes.txt"; : > "$codes"
  i=1
  while [ "$i" -le 12 ]; do
    curl -s -o /dev/null -w '%{http_code}\n' -X POST -H 'Content-Type: application/json' \
      -d '{"username":"m6-ratelimit","password":"wrong-m6"}' "$HTTP/api/login" >> "$codes"
    i=$((i + 1))
  done
  n429=$(grep -c '^429$' "$codes")
  {
    echo "12 次同源登录尝试的状态码："; cat "$codes"
    echo "429 出现次数：$n429"
    echo "（对照：extra-checks.sh 的 SEC-13 是**合流前**的 finding 判据——它要求 429 出现次数为 0；"
    echo "  合流后它必然翻转为 FAIL，这正是 F-08 修好的证据，不是回归。）"
  } >> "$LOG"
  verdict M6-13 "$([ "$n429" -ge 1 ] && echo 0 || echo 1)" \
    "登录限速：12 次同源失败登录里出现 $n429 次 429（F-08 的正判据）"

  # ---------------------------------------------------------------- M6-14 任务生命周期 + WS 广播
  job_log="$EVID/merge-m6-job-e2e.txt"
  sh "$HERE/agent-up.sh" "$NODE_ID" > "$QA_DIR/agent-up-m6.log" 2>&1
  sh "$HERE/job-e2e.sh" > "$job_log" 2>&1
  job_fail=$(grep -c ': FAIL' "$job_log")
  job_pass=$(grep -c ': PASS' "$job_log")
  sh "$HERE/agent-down.sh" >> "$QA_DIR/agent-up-m6.log" 2>&1
  verdict M6-14 "$([ "$job_fail" = 0 ] && [ "$job_pass" -ge 10 ] && echo 0 || echo 1)" \
    "job-e2e（任务生命周期 + 告警 + WebSocket 广播）：PASS=$job_pass FAIL=$job_fail"

  # ---------------------------------------------------------------- M6-15 迁移幂等 + 11 张表
  sh "$HERE/server-down.sh" >> "$QA_DIR/server-up-m6.log" 2>&1
  startsrv
  h2=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$HTTP/api/health")
  tables=$(psql_q "select count(*) from pg_tables where schemaname='public';")
  admin=$(psql_q "select count(*) from users where username='admin';")
  verdict M6-15 "$([ "$h2" = 200 ] && [ "${tables:-0}" -eq 11 ] && [ "${admin:-0}" -eq 1 ] && echo 0 || echo 1)" \
    "重启后 /api/health=$h2、public 表 ${tables:-?} 张（基线 11）、admin 行 ${admin:-?}（要求 1）"
  stopsrv
fi

echo "----" | tee -a "$LOG"
echo "M6-CHECKS: PASS=$PASS FAIL=$FAIL   证据目录: $EVID" | tee -a "$LOG"
exit "$FAIL"
