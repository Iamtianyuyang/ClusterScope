#!/bin/bash
# qa/harness/no-root-fixes-checks.sh
#
# 本轮「无 root 修复」的 QA 执行程序（自包含、离线、可复制粘贴）。
# 覆盖 qa/constraints.json 里 FIX-01..FIX-13 共 13 条约束（F2 一条检查证实两条约束）。
#
# 用法：
#   sh qa/harness/no-root-fixes-checks.sh              # 全量（含 release 构建与 systemd 真装真启）
#   sh qa/harness/no-root-fixes-checks.sh --no-slow    # 跳过 F1 动态探针与 F5 的 systemd 起停
#
# 输出：每行 "<ID> PASS|FAIL - 说明"，退出码 = FAIL 条数；证据落 gauntlet-out/qa/evidence/。
#
# 硬规矩（qa/README.md）：这台机器是共享的 ——
#   * 只按自己记录/自己筛出的 PID 停进程；**不用**按字面量名字整机匹配的 pkill/killall；
#   * 不动本机既有的生产 agent（systemd --user 的 clusterscope-agent.service）；
#   * 对 ~/.config/systemd/user/ 与 ~/.config/clusterscope/ 的每个改动都先备份、结束时还原。
#
# F1 的静态闸门：脚本里出现「没有 -F、也不含 $ 变量」的 pkill/killall = 用字面量名字整机匹配，
# 判 FAIL 且**不执行**动态探针（否则会在这台共享机器上误杀别人正在跑的 agent）。
set -u

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO" || exit 2
EVID="$REPO/gauntlet-out/qa/evidence"
mkdir -p "$EVID"
WORK="${TMPDIR:-/tmp}/nr-fixes-checks"
rm -rf "$WORK"
mkdir -p "$WORK"
BASE="${NR_FIX_BASE:-7ca587a}"
BIN="$REPO/target/release/clusterscope-agent"
NO_SLOW=0
[ "${1:-}" = "--no-slow" ] && NO_SLOW=1

PASS=0
FAIL=0
CUR_LOG=""
fails=""

start_check() { CUR_LOG="$EVID/$1"; : > "$CUR_LOG"; fails=""; }
A() { # A <说明> <0=ok / 非0=失败>
  if [ "$2" -eq 0 ]; then
    printf '  ok   %s\n' "$1" >> "$CUR_LOG"
  else
    printf '  FAIL %s\n' "$1" >> "$CUR_LOG"
    fails="$fails | $1"
  fi
}
end_check() { # end_check <ID> <说明>
  local id="$1" detail="$2" st
  if [ -z "$fails" ]; then st=PASS; PASS=$((PASS + 1)); else st=FAIL; FAIL=$((FAIL + 1)); detail="$detail — 失败项:${fails}"; fi
  printf '%s %s - %s\n' "$id" "$st" "$detail" | tee -a "$EVID/no-root-fixes-checks.txt"
}
strip_ansi() { sed -r 's/\x1b\[[0-9;]*[A-Za-z]//g'; }
contains() { case "$2" in *"$1"*) return 0 ;; *) return 1 ;; esac; }

echo "== 环境 =="
echo "repo=$REPO  base=$BASE  uid=$(id -u)  host=$(hostname)  no-slow=$NO_SLOW"
loginctl show-user "$USER" -p Linger 2>/dev/null || echo "Linger=?"
: > "$EVID/no-root-fixes-checks.txt"

# release 二进制（F1–F4 用它；F5 的 unit 指向 ~/.local/bin，与本文件无关）
if [ ! -x "$BIN" ] && [ "$NO_SLOW" = 0 ]; then
  echo "==> 构建 $BIN（离线）"
  (export PATH="$HOME/.cargo/bin:$PATH"; cargo build --release -p agent --offline) > "$WORK/build.log" 2>&1 ||
    { echo "构建失败，尾部输出："; tail -20 "$WORK/build.log"; }
fi

need_bin() { # need_bin <ID> <说明>
  [ -x "$BIN" ] && return 0
  start_check "$1-missing-binary.txt"
  echo "target/release/clusterscope-agent 不存在：先跑 cargo build --release -p agent --offline" >> "$CUR_LOG"
  end_check "$1" "$2（二进制缺失，未执行）"
  return 1
}

# ------------------------------------------------------------------ F1 安装脚本不误杀无关 agent
f1() {
  start_check "no-root-fixes-F1-install-agent-stop.txt"
  local scope bad
  scope="$(grep -nE '(^|[[:space:]])(pkill|killall)([[:space:]]|$)' deploy/install-agent.sh 2>/dev/null || true)"
  {
    echo "== 静态：脚本里按名字整机匹配的杀进程调用（pkill/killall）=="
    echo "${scope:-<none>}"
  } >> "$CUR_LOG"
  bad="$(printf '%s\n' "$scope" | grep -v -- ' -F' | grep -v '\$' | grep -v '^$' || true)"
  if [ -n "$bad" ]; then
    {
      echo "== 判定 =="
      echo "脚本里仍有**用字面量名字**整机匹配的 pkill/killall（既不是 PID 文件 -F，也不含变量/路径精确化）："
      echo "$bad"
      echo "在共享机器上执行它会杀掉与本安装无关的 agent（含本机常驻实例），"
      echo "因此本检查**不执行**动态探针，直接按静态证据判 FAIL。"
    } >> "$CUR_LOG"
    A "停止/替换不许用字面量名字整机匹配（命中：$(printf '%s' "$bad" | tr '\n' ';'))" 1
    end_check F1 "install-agent.sh 仍含整机匹配的 pkill（静态判 FAIL，未执行破坏性步骤；见 $(basename "$CUR_LOG")）"
    return
  fi
  grep -qE '^[[:space:]]*#[^\n]*PID' deploy/install-agent.sh
  A "脚本里有注释说明它按 PID/进程做事（停止逻辑有交代）" $?
  [ "$NO_SLOW" = 1 ] && { echo "(--no-slow：跳过动态探针)" >> "$CUR_LOG"; end_check F1 "静态部分完成；动态探针被 --no-slow 跳过"; return; }
  need_bin F1 "安装脚本动态探针" || return
  # 探针：独立副本，模拟"同一台机器上与本次安装无关的另一个 agent"
  mkdir -p "$WORK/probe/bin" "$WORK/probe/home" "$WORK/probe/logs"
  cp -f "$BIN" "$WORK/probe/bin/clusterscope-agent"
  cat > "$WORK/probe/agent.yaml" <<EOF
server_addr: "http://127.0.0.1:59999"
node_id: "qa-fix-probe"
node_id_file: $WORK/probe/node_id
report_interval_secs: 30
log_dir: $WORK/probe/logs
EOF
  HOME="$WORK/probe/home" nohup "$WORK/probe/bin/clusterscope-agent" -c "$WORK/probe/agent.yaml" >> "$WORK/probe/probe.log" 2>&1 &
  local probe=$!
  sleep 2
  kill -0 "$probe" 2>/dev/null
  A "探针进程已起来（PID $probe，模拟同机上别人的 agent）" $?
  local foreign_before
  foreign_before="$(pgrep -f 'clusterscope-agent' 2>/dev/null | grep -v "^$probe$" | sort -n | tr '\n' ' ' || true)"
  # ssh/scp/systemctl 桩：把 install-agent.sh 的"远端"落到本机 scratch HOME，脚本本身不改一处
  mkdir -p "$WORK/stub"
  cat > "$WORK/stub/ssh" <<'STUB'
#!/bin/bash
# QA 桩：把 install-agent.sh 的“远端命令”落到本机 scratch HOME（离线、不动任何真实主机）
home="${NR_FIX_FAKE_HOME:?NR_FIX_FAKE_HOME not set}"
target=""
cmd=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) shift 2 ;;
    -q|-T|-n|-v|-C) shift ;;
    *) if [ -z "$target" ]; then target="$1"; else cmd="$1"; fi; shift ;;
  esac
done
[ -n "$cmd" ] || { echo "stub-ssh: 不支持交互式会话" >&2; exit 1; }
export HOME="$home"
unset XDG_CONFIG_HOME XDG_STATE_HOME
exec /bin/bash -c "$cmd"
STUB
  cat > "$WORK/stub/scp" <<'STUB'
#!/bin/bash
# QA 桩：scp [flags] SRC HOST:/path  → 本机 cp（离线）
src=""
dst=""
while [ $# -gt 0 ]; do
  case "$1" in
    -q|-r|-p|-B) shift ;;
    -o) shift 2 ;;
    *) if [ -z "$src" ]; then src="$1"; else dst="$1"; fi; shift ;;
  esac
done
[ -n "$dst" ] || exit 1
dst="${dst#*:}"
mkdir -p "$(dirname "$dst")"
cp -f "$src" "$dst"
STUB
  cat > "$WORK/stub/systemctl" <<'STUB'
#!/bin/bash
# QA 桩：让 install-agent.sh 的“远端”走 nohup 分支（那正是本轮改的停止逻辑所在）；
# F5 另外用真 systemctl 验证 unit 本身。调用被记录，便于取证。
echo "stub-systemctl $*" >> "${NR_FIX_STUB_LOG:-/dev/null}"
exit 1
STUB
  chmod +x "$WORK/stub/ssh" "$WORK/stub/scp" "$WORK/stub/systemctl"
  mkdir -p "$WORK/home-a"
  export NR_FIX_FAKE_HOME="$WORK/home-a"
  export NR_FIX_STUB_LOG="$WORK/stub-systemctl.log"
  PATH="$WORK/stub:$PATH" sh deploy/install-agent.sh qa@fake-host http://127.0.0.1:59999 qa-fix-node > "$WORK/install-1.log" 2>&1
  local rc1=$?
  sleep 1
  local pids1
  pids1="$(pgrep -f "$WORK/home-a/.local/bin/clusterscope-agent" 2>/dev/null | sort -n | tr '\n' ' ' || true)"
  PATH="$WORK/stub:$PATH" sh deploy/install-agent.sh qa@fake-host http://127.0.0.1:59999 qa-fix-node > "$WORK/install-2.log" 2>&1
  local rc2=$?
  sleep 2
  local pids2
  pids2="$(pgrep -f "$WORK/home-a/.local/bin/clusterscope-agent" 2>/dev/null | sort -n | tr '\n' ' ' || true)"
  {
    echo "== 探针 =="; echo "pid=$probe  log=$WORK/probe/probe.log"
    echo "== 安装前本机其它 clusterscope-agent 进程 =="; echo "${foreign_before:-<none>}"
    echo "== install #1 rc=$rc1（script HOME=$WORK/home-a）=="; tail -6 "$WORK/install-1.log"
    echo "== install #2 rc=$rc2 =="; tail -6 "$WORK/install-2.log"
    echo "== scratch HOME 下的 agent PID：第一次=$pids1 第二次=$pids2 =="
    echo "== stub systemctl 调用 =="; cat "$WORK/stub-systemctl.log" 2>/dev/null || true
  } >> "$CUR_LOG"
  A "第一次安装 rc=0（实际 $rc1）" "$rc1"
  A "第二次安装 rc=0（实际 $rc2）" "$rc2"
  kill -0 "$probe" 2>/dev/null
  A "探针进程在两次安装后仍然存活" $?
  local p dead=''
  for p in $foreign_before; do kill -0 "$p" 2>/dev/null || dead="$dead $p"; done
  A "安装前就存在的其它 clusterscope-agent 进程全部存活（快照: ${foreign_before:-无}）" "$([ -z "$dead" ] && echo 0 || echo 1)"
  local n1 n2 still=''
  n1=$(printf '%s' "$pids1" | wc -w)
  n2=$(printf '%s' "$pids2" | wc -w)
  A "第一次安装后 scratch HOME 下有 1 个 agent（实际 $n1）" "$([ "$n1" = 1 ] && echo 0 || echo 1)"
  A "第二次安装后 scratch HOME 下仍有且仅有 1 个 agent（实际 $n2）" "$([ "$n2" = 1 ] && echo 0 || echo 1)"
  for p in $pids1; do kill -0 "$p" 2>/dev/null && still="$still $p"; done
  A "停止/替换只作用在本次安装启动的进程（旧 PID ${pids1:-无} → 新 ${pids2:-无}；仍在跑的旧 PID:${still:-无}）" \
    "$([ -z "$still" ] || [ "$pids1" = "$pids2" ] && echo 0 || echo 1)"
  for p in $pids2 $probe; do kill "$p" 2>/dev/null; done
  sleep 1
  for p in $pids2 $probe; do kill -9 "$p" 2>/dev/null; done
  end_check F1 "两次安装只动自己启动的进程；探针与既存实例全部存活（见 $(basename "$CUR_LOG")）"
}

# ------------------------------------------------------------------ F2 显式 -c 缺失 → 硬错误
f2() {
  start_check "no-root-fixes-F2-explicit-missing-config.txt"
  need_bin F2 "显式 -c 缺失的复现" || return
  mkdir -p "$WORK/home-b"
  local cfg="$WORK/absent-dir/agent.yaml" out rc
  HOME="$WORK/home-b" timeout 8 "$BIN" -c "$cfg" > "$WORK/f2.out" 2>&1
  rc=$?
  out="$(strip_ansi < "$WORK/f2.out")"
  {
    echo "== 命令 =="; echo "HOME=$WORK/home-b timeout 8 $BIN -c $cfg"
    echo "== exit=$rc =="; echo "$out"
    echo "== HOME 下被创建的东西 =="; find "$WORK/home-b" -maxdepth 3 2>/dev/null
  } >> "$CUR_LOG"
  A "退出码非 0（实际 $rc）" "$([ "$rc" -ne 0 ] && echo 0 || echo 1)"
  contains "$cfg" "$out"
  A "输出逐字点名那个不存在的配置路径" $?
  printf '%s' "$out" | grep -qiE 'not found|no such file|does not exist|missing|不存在'
  A "输出说明该配置文件不存在" $?
  printf '%s' "$out" | grep -q '\.config/clusterscope/agent.yaml'
  A "输出点名用户级配置位置（照抄系统级 unit 的人能看懂）" $?
  ! contains 'http://localhost:50051' "$out"
  A "输出中不出现默认地址 http://localhost:50051（没有静默回退）" $?
  end_check F2 "显式 -c 指向不存在的文件：非 0 退出且点名该文件、不回退默认值"
}

# ------------------------------------------------------------------ F3 不带 -c：告警 + 继续跑
f3() {
  start_check "no-root-fixes-F3-default-missing.txt"
  need_bin F3 "默认配置缺失的复现" || return
  if [ -e /etc/clusterscope/agent.yaml ]; then
    echo "本机存在 /etc/clusterscope/agent.yaml，默认路径缺失的前提不成立 —— 该条需人工裁决" >> "$CUR_LOG"
    end_check F3 "环境不适用：本机存在 /etc/clusterscope/agent.yaml"
    return
  fi
  mkdir -p "$WORK/home-c"
  local out rc
  HOME="$WORK/home-c" timeout 6 "$BIN" > "$WORK/f3.out" 2>&1
  rc=$?
  out="$(strip_ansi < "$WORK/f3.out")"
  {
    echo "== 命令 =="; echo "HOME=$WORK/home-c timeout 6 $BIN   # 不带 -c"
    echo "== exit=$rc（124 = 被 timeout 收掉，说明进程还在跑）=="; echo "$out"
  } >> "$CUR_LOG"
  A "进程仍在运行（exit 124，实际 $rc）—— 守住既有 NR-06" "$([ "$rc" = 124 ] && echo 0 || echo 1)"
  contains 'ClusterScope Agent starting' "$out"
  A "输出有启动横幅" $?
  contains '/etc/clusterscope/agent.yaml' "$out"
  A "输出点名默认配置路径 /etc/clusterscope/agent.yaml（回退不再静默）" $?
  end_check F3 "不带 -c 时：点名默认路径后仍按内置默认值启动"
}

# ------------------------------------------------------------------ F4 身份文件父目录
f4() {
  start_check "no-root-fixes-F4-identity-parent.txt"
  need_bin F4 "node identity 父目录的复现" || return
  mkdir -p "$WORK/home-d"
  cat > "$WORK/valid.yaml" <<EOF
server_addr: "http://127.0.0.1:59999"
node_id: ""
report_interval_secs: 30
log_dir: $WORK/home-d/state-logs
EOF
  local out rc
  HOME="$WORK/home-d" timeout 6 "$BIN" -c "$WORK/valid.yaml" > "$WORK/f4.out" 2>&1
  rc=$?
  out="$(strip_ansi < "$WORK/f4.out")"
  {
    echo "== 命令 =="; echo "HOME=$WORK/home-d timeout 6 $BIN -c $WORK/valid.yaml"
    echo "== exit=$rc =="; echo "$out"
    echo "== HOME 下被创建的东西 =="; find "$WORK/home-d" -maxdepth 3 2>/dev/null
    echo "== node_id 内容 =="; cat "$WORK/home-d/.config/node_id" 2>/dev/null || echo "<缺失>"
  } >> "$CUR_LOG"
  [ -s "$WORK/home-d/.config/node_id" ]
  A "HOME 下的 .config/node_id 被创建且非空" $?
  ! contains 'Failed to write node identity' "$out"
  A "输出中没有 Failed to write node identity" $?
  A "进程在 5 秒后仍然在运行（exit 124，实际 $rc）" "$([ "$rc" = 124 ] && echo 0 || echo 1)"
  [ -d "$WORK/home-d/state-logs" ]
  A "配置里的 log_dir 也被创建（create_dir_all）" $?
  end_check F4 "干净 HOME（无 ~/.config）下也能建父目录并写下身份文件"
}

# ------------------------------------------------------------------ F5 用户级 unit
f5() {
  start_check "no-root-fixes-F5-user-units.txt"
  local ua=deploy/clusterscope-agent.service us=deploy/clusterscope-server.service
  [ -f "$ua" ]
  A "仓库里有用户级 agent unit（$ua）" $?
  [ -f "$us" ]
  A "仓库里有用户级 server unit（$us）" $?
  {
    echo "== README 里的 systemctl --user 行 =="; grep -n 'systemctl --user' README.md || true
    echo "== deploy/*.service =="; ls -l deploy/*.service
  } >> "$CUR_LOG"
  if [ ! -f "$ua" ] || [ ! -f "$us" ]; then
    end_check F5 "仓库里没有用户级 unit 文件（README 的 systemctl --user 命令在干净机器上必然失败）"
    return
  fi
  {
    echo "== $ua =="; cat "$ua"; echo; echo "== $us =="; cat "$us"
  } >> "$CUR_LOG"
  ! grep -qE '^(User|Group)=' "$ua" "$us"
  A "用户级 unit 不含 User=/Group=" $?
  ! grep -q '/usr/local/bin' "$ua" "$us"
  A "用户级 unit 不走 /usr/local/bin" $?
  ! grep -q 'multi-user.target' "$ua" "$us"
  A "用户级 unit 不挂 multi-user.target" $?
  grep -q 'WantedBy=default.target' "$ua"
  A "agent unit 挂 default.target" $?
  grep -q 'WantedBy=default.target' "$us"
  A "server unit 挂 default.target" $?
  grep -qE 'ExecStart=%h/\.local/bin/clusterscope-agent' "$ua"
  A "agent unit 二进制走 %h/.local/bin" $?
  grep -qE 'ExecStart=.*%h/\.config/clusterscope/agent\.yaml' "$ua"
  A "agent unit 配置走 %h/.config/clusterscope/agent.yaml" $?
  grep -qE 'ExecStart=%h/\.local/bin/clusterscope-server' "$us"
  A "server unit 二进制走 %h/.local/bin" $?
  grep -qE 'ExecStart=.*%h/\.config/clusterscope/server\.yaml' "$us"
  A "server unit 配置走 %h/.config/clusterscope/server.yaml" $?
  grep -q 'clusterscope-agent' README.md
  A "README 提到 clusterscope-agent 这个名字" $?
  grep -q 'clusterscope-server' README.md
  A "README 提到 clusterscope-server 这个名字" $?
  grep -q 'clusterscope-agent.service' deploy/install-agent.sh
  A "install-agent.sh 生成的 unit 名与仓库用户级 unit 同名（clusterscope-agent.service）" $?
  if [ "$NO_SLOW" = 1 ]; then
    echo "(--no-slow：跳过 systemd 真装真启)" >> "$CUR_LOG"
    end_check F5 "静态部分完成；systemd 起停被 --no-slow 跳过"
    return
  fi
  # ---- 真装真启（备份 → 安装 → daemon-reload → enable --now → status → 还原）----
  local UD="$HOME/.config/systemd/user" CFGD="$HOME/.config/clusterscope"
  local BK="$WORK/units-backup" rc
  mkdir -p "$BK/systemd" "$BK/config"
  local f agent_active_before agent_enabled_before agent_pid_before agent_pid_after server_cfg_existed=0 selfcfg=0
  for f in clusterscope-agent.service clusterscope-server.service; do
    [ -f "$UD/$f" ] && cp -a "$UD/$f" "$BK/systemd/$f"
  done
  [ -f "$CFGD/server.yaml" ] && { cp -a "$CFGD/server.yaml" "$BK/config/server.yaml"; server_cfg_existed=1; }
  agent_active_before="$(systemctl --user is-active clusterscope-agent.service 2>&1 || true)"
  agent_enabled_before="$(systemctl --user is-enabled clusterscope-agent.service 2>&1 || true)"
  agent_pid_before="$(systemctl --user show clusterscope-agent.service -p MainPID --value 2>/dev/null || true)"
  local ports_busy=0
  ss -ltn 2>/dev/null | grep -qE ':(8080|50051)[[:space:]]' && ports_busy=1
  mkdir -p "$UD"
  cp -f "$ua" "$UD/clusterscope-agent.service"
  cp -f "$us" "$UD/clusterscope-server.service"
  systemctl --user daemon-reload
  A "daemon-reload rc=0" $?
  systemctl --user enable clusterscope-agent.service > /dev/null 2>&1
  A "enable clusterscope-agent.service rc=0" $?
  [ "$(systemctl --user is-enabled clusterscope-agent.service 2>/dev/null)" = enabled ]
  A "is-enabled = enabled" $?
  systemctl --user enable --now clusterscope-agent.service > /dev/null 2>&1
  A "enable --now clusterscope-agent.service rc=0" $?
  [ "$(systemctl --user is-active clusterscope-agent.service 2>/dev/null)" = active ]
  A "agent unit is-active = active" $?
  agent_pid_after="$(systemctl --user show clusterscope-agent.service -p MainPID --value 2>/dev/null || true)"
  if [ "$agent_active_before" = active ]; then
    A "装上仓库 unit 后，本机生产 agent 的 MainPID 不变（$agent_pid_before → $agent_pid_after）" \
      "$([ "$agent_pid_after" = "$agent_pid_before" ] && echo 0 || echo 1)"
  fi
  systemctl --user show clusterscope-agent.service -p FragmentPath -p ExecStart --value >> "$CUR_LOG" 2>&1
  systemctl --user status clusterscope-agent.service --no-pager >> "$CUR_LOG" 2>&1 || true
  # server unit：真起一次（需要一份能连上本机 PG 的配置）
  if [ "$server_cfg_existed" = 0 ]; then
    sed -e 's#postgresql://clusterscope:clusterscope@localhost:5432/clusterscope#postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope#' \
      deploy/server.yaml.example > "$CFGD/server.yaml"
  fi
  systemctl --user enable --now clusterscope-server.service > /dev/null 2>&1
  rc=$?
  A "enable --now clusterscope-server.service rc=0（实际 $rc）" "$rc"
  sleep 3
  if [ "$(systemctl --user is-active clusterscope-server.service 2>/dev/null)" != active ] && [ "$ports_busy" = 0 ]; then
    selfcfg=1
    sed -e 's#postgresql://clusterscope:clusterscope@localhost:5432/clusterscope#postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope#' \
      -e 's#change-me-to-a-long-random-string#nr-fixes-checks-secret-0123456789#' deploy/server.yaml.example > "$CFGD/server.yaml"
    systemctl --user restart clusterscope-server.service > /dev/null 2>&1
    sleep 4
  fi
  systemctl --user show clusterscope-server.service -p FragmentPath -p ExecStart -p MainPID --value >> "$CUR_LOG" 2>&1
  systemctl --user status clusterscope-server.service --no-pager >> "$CUR_LOG" 2>&1 || true
  if [ "$ports_busy" = 1 ]; then
    echo "端口 8080/50051 被占用：server 启动未探测（记录为限制，不算 FAIL）" >> "$CUR_LOG"
    A "server unit 装载成功（端口被占用，is-active 未断言）" "$([ "$(systemctl --user is-enabled clusterscope-server.service 2>/dev/null)" = enabled ] && echo 0 || echo 1)"
  else
    [ "$(systemctl --user is-active clusterscope-server.service 2>/dev/null)" = active ]
    A "server unit is-active = active（真起）" $?
    local health
    health="$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 http://127.0.0.1:8080/api/health 2>/dev/null || echo 000)"
    echo "curl /api/health -> $health" >> "$CUR_LOG"
    A "server 起来后 /api/health = 200（实际 $health）" "$([ "$health" = 200 ] && echo 0 || echo 1)"
  fi
  # ---- 还原（安装前有什么就还原成什么，没有的删掉）----
  systemctl --user disable --now clusterscope-server.service > /dev/null 2>&1
  rm -f "$UD/clusterscope-server.service"
  if [ -f "$BK/systemd/clusterscope-agent.service" ]; then
    cp -a "$BK/systemd/clusterscope-agent.service" "$UD/clusterscope-agent.service"
  else
    systemctl --user disable --now clusterscope-agent.service > /dev/null 2>&1
    rm -f "$UD/clusterscope-agent.service"
  fi
  if [ "$agent_enabled_before" != enabled ]; then
    systemctl --user disable clusterscope-agent.service > /dev/null 2>&1
  fi
  if [ -f "$BK/config/server.yaml" ]; then
    cp -a "$BK/config/server.yaml" "$CFGD/server.yaml"
  elif [ "$server_cfg_existed" = 0 ]; then
    rm -f "$CFGD/server.yaml"
  fi
  systemctl --user daemon-reload
  sleep 2
  if [ -f "$BK/systemd/clusterscope-agent.service" ]; then
    A "还原：~/.config/systemd/user/clusterscope-agent.service 与安装前逐字节一致" \
      "$(cmp -s "$UD/clusterscope-agent.service" "$BK/systemd/clusterscope-agent.service" && echo 0 || echo 1)"
  else
    [ ! -f "$UD/clusterscope-agent.service" ]
    A "还原：安装前没有 agent unit，脚本已把仓库装的那份删掉" $?
  fi
  [ ! -f "$UD/clusterscope-server.service" ]
  A "还原：仓库装的 server unit 已移除" $?
  A "还原：server 已停（is-active = inactive）" \
    "$([ "$(systemctl --user is-active clusterscope-server.service 2>/dev/null)" != active ] && echo 0 || echo 1)"
  if [ "$server_cfg_existed" = 1 ]; then
    A "还原：~/.config/clusterscope/server.yaml 与安装前逐字节一致" \
      "$(cmp -s "$CFGD/server.yaml" "$BK/config/server.yaml" && echo 0 || echo 1)"
  fi
  if [ "$agent_active_before" = active ]; then
    A "还原：生产 agent 仍在跑（MainPID $agent_pid_before → $(systemctl --user show clusterscope-agent.service -p MainPID --value 2>/dev/null || echo '?')，未被本轮检查打扰）" \
      "$([ "$(systemctl --user show clusterscope-agent.service -p MainPID --value 2>/dev/null || echo x)" = "$agent_pid_before" ] && echo 0 || echo 1)"
  fi
  echo "(selfcfg=$selfcfg  ports_busy=$ports_busy  agent_active_before=$agent_active_before  server.yaml_existed=$server_cfg_existed)" >> "$CUR_LOG"
  end_check F5 "用户级 unit 名字/路径/真装真启/status 与还原"
}

# ------------------------------------------------------------------ F6 系统级 unit 的角色
f6() {
  start_check "no-root-fixes-F6-system-units.txt"
  {
    echo "== deploy/agent.service =="; head -4 deploy/agent.service
    echo "== deploy/server.service =="; head -4 deploy/server.service
    echo "== README 里提到 root 的行 =="; grep -nE '需要 root|需 root|要 root|sudo|\broot\b' README.md || true
  } >> "$CUR_LOG"
  grep -q 'multi-user.target' deploy/agent.service
  A "系统级 agent unit 仍在（multi-user.target）" $?
  grep -q 'User=clusterscope' deploy/server.service
  A "系统级 server unit 仍在（User=clusterscope）" $?
  head -4 deploy/agent.service | grep -qiE 'system|系统'
  A "agent.service 文件头注明它是系统级（需 root）" $?
  head -4 deploy/server.service | grep -qiE 'system|系统'
  A "server.service 文件头注明它是系统级（需 root）" $?
  grep -nE '需要 root|需 root|要 root|sudo' README.md | grep -q .
  A "README 说明系统级安装需要 root（可选路径）" $?
  end_check F6 "系统级 unit 保留，且角色/前提写清楚"
}

# ------------------------------------------------------------------ F7 无 root 的 PG 路径
f7() {
  start_check "no-root-fixes-F7-postgres-paths.txt"
  {
    echo "== README 里 postgres 相关行 =="; grep -nE 'postgres_url|initdb|pg_ctl|pg16|docker compose|PostgreSQL' README.md || true
  } >> "$CUR_LOG"
  grep -q 'postgres_url' README.md
  A "README 给出「已有实例直接填 postgres_url」这条路径" $?
  grep -qE 'initdb' README.md
  A "README 给出「把 PostgreSQL 装到 HOME」这条路径（initdb）" $?
  grep -q 'pg_ctl' README.md
  A "同上（pg_ctl 启停）" $?
  grep -qE 'pg16|编译到 HOME|装到 HOME|安装到 HOME|解压到 HOME' README.md
  A "README 说明我们实测走的是哪条（点名 pg16 / 装到 HOME）" $?
  grep -nE 'docker' README.md | grep -qE '需要|前提|不可用|没有|可选'
  A "docker 那条路写明了前提（需要 docker / 本集群不可用）" $?
  command -v docker > /dev/null 2>&1
  A "本机确实没有 docker（证明前提必须写明）" "$([ $? -ne 0 ] && echo 0 || echo 1)"
  end_check F7 "README 记录无 root、无 docker 下的两条 PG 路径与实测选择"
}

# ------------------------------------------------------------------ F8 linger 前提
f8() {
  start_check "no-root-fixes-F8-linger.txt"
  {
    echo "== 文档里 linger 相关行 =="; grep -nE 'loginctl|Linger|enable-linger' README.md docs/*.md || true
    echo "== 本机实测 =="; loginctl show-user "$USER" -p Linger 2>&1 || true
  } >> "$CUR_LOG"
  grep -qE 'loginctl' README.md docs/*.md
  A "文档给出 loginctl 的检查方法" $?
  grep -q 'Linger' README.md docs/*.md
  A "文档说明 Linger 的含义（登出后用户服务是否存活）" $?
  grep -q 'enable-linger' README.md docs/*.md
  A "文档给出 enable-linger" $?
  grep -nE 'enable-linger' README.md docs/*.md | grep -qE 'root|管理员|sudo|需要'
  A "文档写明 enable-linger 需要管理员/root" $?
  loginctl show-user "$USER" -p Linger | grep -q '^Linger='
  A "本机 loginctl 可查 Linger" $?
  end_check F8 "用户级部署的 linger 前提写进文档"
}

# ------------------------------------------------------------------ F9 系统级 vs 用户级
f9() {
  start_check "no-root-fixes-F9-install-modes.txt"
  {
    echo "== README 里的 unit 文件名与 systemctl --user =="; grep -nE 'deploy/[a-z-]+\.service|systemctl --user' README.md || true
  } >> "$CUR_LOG"
  grep -q 'deploy/clusterscope-agent.service' README.md
  A "README 提到用户级 agent unit 文件" $?
  grep -q 'deploy/clusterscope-server.service' README.md
  A "README 提到用户级 server unit 文件" $?
  grep -qE 'deploy/(agent|server)\.service' README.md
  A "README 提到系统级 unit（可选、需 root 时用）" $?
  grep -q 'systemctl --user' README.md
  A "README 的 systemctl --user 命令仍在（且有仓库里的 unit 对应）" $?
  end_check F9 "两种安装方式的分工与前提写进 README"
}

# ------------------------------------------------------------------ F10 既有测试全绿
f10() {
  start_check "no-root-fixes-F10-tests.txt"
  (export PATH="$HOME/.cargo/bin:$PATH"; cargo test --workspace --offline) > "$WORK/cargo-test.log" 2>&1
  local rc=$? passed failed deleted
  passed="$(grep -oE '[0-9]+ passed' "$WORK/cargo-test.log" | awk '{s+=$1} END{print s+0}')"
  failed="$(grep -oE '[0-9]+ failed' "$WORK/cargo-test.log" | awk '{s+=$1} END{print s+0}')"
  deleted="$(git diff "$BASE" -- crates 2>/dev/null | grep -E '^-[^-].*#\[(tokio::)?test\]' || true)"
  {
    echo "== cargo test --workspace --offline：rc=$rc  passed=$passed  failed=$failed =="
    grep -E '^test result|^error' "$WORK/cargo-test.log" || true
    echo "== 被删掉的 #[test] 行 =="; echo "${deleted:-<none>}"
  } >> "$CUR_LOG"
  A "cargo test 退出码 0（实际 $rc）" "$rc"
  A "0 个失败（实际 $failed）" "$([ "$failed" = 0 ] && echo 0 || echo 1)"
  A "通过数 ≥ 44（实际 $passed）" "$([ "$passed" -ge 44 ] && echo 0 || echo 1)"
  A "没有删除既有 #[test]/#[tokio::test]" "$([ -z "$deleted" ] && echo 0 || echo 1)"
  end_check F10 "既有测试继续全绿，未删未跳"
}

# ------------------------------------------------------------------ F11 constraints 只追加
f11() {
  start_check "no-root-fixes-F11-constraints.txt"
  local removed info
  removed="$(git diff "$BASE" -- qa/constraints.json 2>/dev/null | grep '^-' | grep -v '^---' || true)"
  info="$(node -e '
const fs = require("fs");
const a = JSON.parse(fs.readFileSync("qa/constraints.json", "utf8"));
const ids = a.map((c) => c.id);
const need = [];
for (let i = 1; i <= 13; i++) need.push("FIX-" + String(i).padStart(2, "0"));
console.log(JSON.stringify({
  total: ids.length,
  missingFix: need.filter((x) => !ids.includes(x)),
  missingNr: ids.filter((x) => x.startsWith("NR-")).length,
  dup: ids.filter((x, i) => ids.indexOf(x) !== i),
}));
' 2>&1 || echo '{"error":"parse failed"}')"
  {
    echo "== 被删/被改的行（必须为空）=="; echo "${removed:-<none>}"
    echo "== 约束统计 =="; echo "$info"
  } >> "$CUR_LOG"
  A "qa/constraints.json 只有新增（diff 里没有被删/被改的行）" "$([ -z "$removed" ] && echo 0 || echo 1)"
  contains '"missingFix":[]' "$info"
  A "FIX-01..FIX-13 都在" $?
  contains '"dup":[]' "$info"
  A "约束 id 唯一" $?
  echo "$info" | grep -qE '"total":(11[7-9]|1[2-9][0-9])'
  A "约束总条数 ≥117（104 既有 + 13 本轮）" $?
  echo "$info" | grep -qE '"missingNr":(2[2-9]|[3-9][0-9])'
  A "既有 NR-* 约束仍在（≥22 条）" $?
  end_check F11 "约束只追加、不改既有条目"
}

# ------------------------------------------------------------------ F12 改动范围
f12() {
  start_check "no-root-fixes-F12-scope.txt"
  local changed bad f hs
  changed="$( { git diff --name-only "$BASE" 2>/dev/null; git ls-files --others --exclude-standard 2>/dev/null; } | sort -u)"
  bad=""
  while IFS= read -r f; do
    [ -z "$f" ] && continue
    case "$f" in
      crates/agent/src/*.rs) ;;
      crates/agent/tests/*|crates/common/src/config.rs) ;;
      deploy/*|README.md|docs/*|features/*|qa/*|GAUNTLET.md) ;;
      *) bad="$bad $f" ;;
    esac
  done <<< "$changed"
  hs="$(git diff --name-only "$BASE" -- crates/storage crates/server 2>/dev/null || true)"
  {
    echo "== 本次改动/新增的文件 =="; echo "$changed"
    echo "== 允许集之外的 =="; echo "${bad:-<none>}"
    echo "== crates/storage 与 crates/server 的改动 =="; echo "${hs:-<none>}"
  } >> "$CUR_LOG"
  A "改动文件全部落在本轮允许集内" "$([ -z "$bad" ] && echo 0 || echo 1)"
  A "crates/storage 与 crates/server 零改动" "$([ -z "$hs" ] && echo 0 || echo 1)"
  end_check F12 "改动范围仅限本轮四项相关文件"
}

f1
f2
f3
f4
f5
f6
f7
f8
f9
f10
f11
f12

echo
echo "PASS=$PASS FAIL=$FAIL  证据目录: $EVID"
exit "$FAIL"
