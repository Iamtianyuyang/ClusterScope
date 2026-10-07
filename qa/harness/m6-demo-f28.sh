#!/bin/sh
# qa/harness/m6-demo-f28.sh -- F-28 的独立实验驱动（demo/16 用的可回放版本）
#
# 用法: sh qa/harness/m6-demo-f28.sh clippy|serial|parallel|narrowed
#
#   clippy    判据：cargo clippy --workspace --all-targets -- -D warnings 必须 rc=0
#   serial    工作树构建的验收测试二进制，--test-threads=1（基线）
#   parallel  同一二进制 --test-threads=8 跑 3 次（互斥语义是否保留）
#   narrowed  对拍：把 guard 收窄成同步块（唯一能让 await_holding_lock 闭嘴的 std 写法）
#             的变体，在 8 线程下必须自己踩自己 —— 用来证明「不能收窄」
#
# 铁律：产品代码只读。对拍变体由 `git archive` 导出到 gauntlet-out/qa7/probe-src 后再改，
#       工作树自始至终 git status 干净（不碰任何已提交文件）。
# 共享机器纪律：本脚本不起 server/agent，也不需要停别人的进程。
#
# 坑（本脚本第一版踩过，记在这里）：deps/ 里同时躺着工作树的产物和对拍变体的产物，
# 用 `ls -t | head -1` 选「最新」会选到变体（它更晚构建）——必须用 cargo 自己解析路径。
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/env.sh" || exit 2
export POSTGRES_URL="$PGURL"

CMD="${1:-}"
OUT="$REPO/gauntlet-out/qa7"
SRC="$OUT/probe-src"
mkdir -p "$OUT"

exe_of() { # exe_of <message-format=json 日志> -- 打出第一个测试可执行文件路径
  python3 - "$1" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    line = line.strip()
    if not line.startswith("{"):
        continue
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") \
       and m.get("target", {}).get("name") == "audit_queries_acceptance":
        print(m["executable"]); break
PY
}

real_bin() {
  ( cd "$REPO" && CARGO_TARGET_DIR="$REPO/target" \
      cargo test --test audit_queries_acceptance --offline --no-run --message-format=json ) \
      > "$OUT/demo-real-build.json" 2>&1 || return 1
  exe_of "$OUT/demo-real-build.json"
}

case "$CMD" in
clippy)
  echo "# 判据：--all-targets（含测试目标）也要没有一条告警（round 2 这里打印 9 条 await_holding_lock）"
  ( cd "$REPO" && cargo clippy --workspace --all-targets --offline -- -D warnings )
  rc=$?
  echo "clippy rc=$rc"
  exit "$rc"
  ;;
serial)
  B=$(real_bin)
  [ -n "$B" ] && [ -x "$B" ] || { echo "找不到工作树的测试二进制"; exit 2; }
  echo "# 工作树（HEAD=$(git -C "$REPO" rev-parse --short HEAD)）的验收测试二进制: $B"
  echo "#   sha256=$(sha256sum "$B" | awk '{print $1}')"
  "$B" --test-threads=1
  rc=$?
  echo "rc=$rc"
  exit "$rc"
  ;;
parallel)
  B=$(real_bin)
  [ -n "$B" ] && [ -x "$B" ] || { echo "找不到工作树的测试二进制"; exit 2; }
  echo "# 同一二进制（sha256=$(sha256sum "$B" | awk '{print $1}' | cut -c1-16)…），8 线程并行跑 4 条共用 m6- 夹具的测试"
  fail=0
  i=1
  while [ "$i" -le 3 ]; do
    "$B" --test-threads=8 > "$OUT/demo-parallel-$i.txt" 2>&1
    rc=$?
    echo "run$i: $(grep -E '^test result' "$OUT/demo-parallel-$i.txt")  (rc=$rc)"
    [ "$rc" -eq 0 ] || fail=1
    i=$((i + 1))
  done
  exit "$fail"
  ;;
narrowed)
  REF="${F28_REF:-HEAD}"
  echo "# 对拍：$REF 的同一份源码，只把 guard 收窄成 { let _guard = lock_fixture(); }"
  echo "# （这是 std::sync::Mutex 下唯一能让 clippy 闭嘴的写法：guard 不跨 await 点）"
  rm -rf "$SRC"; mkdir -p "$SRC"
  git -C "$REPO" archive "$REF" | tar -x -C "$SRC" || exit 2
  python3 - "$SRC" <<'PY'
import sys, pathlib
root = pathlib.Path(sys.argv[1])
subs = [
    ("use tokio::sync::{Mutex, MutexGuard};", "use std::sync::{Mutex, MutexGuard};"),
    ("static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());",
     "static FIXTURE_LOCK: Mutex<()> = Mutex::new(());"),
    ("/// The guard has to span the whole test body -- the database awaits included --",
     "/// COUNTERFACTUAL: scope-narrowed variant --"),
    ("/// so the lock is the async-aware one: a `std::sync` guard held across an await",
     "/// the guard is taken and dropped in a synchronous block, so no guard is ever"),
    ("/// point blocks the runtime thread the test runs on.",
     "/// held across an await point."),
    ("async fn lock_fixture() -> MutexGuard<'static, ()> {",
     "fn lock_fixture() -> MutexGuard<'static, ()> {"),
    ("    FIXTURE_LOCK.lock().await",
     "    FIXTURE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())"),
    ("    let _guard = lock_fixture().await;", "    { let _guard = lock_fixture(); }"),
]
bad = 0
for name in ("audit_queries_acceptance.rs", "user_queries_acceptance.rs"):
    p = root / "crates/storage/tests" / name
    text = p.read_text()
    for old, new in subs:
        n = text.count(old)
        print("patch %s: %d x %s" % (name, n, old.strip()[:56]))
        if n == 0:
            bad = 1
        text = text.replace(old, new)
    p.write_text(text)
sys.exit(3 if bad else 0)
PY
  prc=$?
  [ "$prc" -eq 0 ] || { echo "对拍变体过期：测试文件的形状变了，替换没全部命中（python rc=$prc）"; exit 3; }

  echo "# 构建变体（CARGO_TARGET_DIR 复用本仓库 target/，只重编 storage 测试目标）"
  ( cd "$SRC" && CARGO_TARGET_DIR="$REPO/target" \
      cargo test --test audit_queries_acceptance --offline --no-run --message-format=json ) \
      > "$OUT/demo-narrow-build.json" 2>&1 || { echo "变体构建失败"; exit 2; }
  P=$(exe_of "$OUT/demo-narrow-build.json")
  [ -n "$P" ] && [ -x "$P" ] || { echo "找不到变体二进制"; exit 2; }
  echo "# 变体二进制: $P"
  echo "#   sha256=$(sha256sum "$P" | awk '{print $1}')"

  "$P" --test-threads=1 > "$OUT/demo-narrow-serial.txt" 2>&1
  echo "收窄后 --test-threads=1: $(grep -E '^test result' "$OUT/demo-narrow-serial.txt")"

  "$P" --test-threads=8 > "$OUT/demo-narrow-run.txt" 2>&1
  rc=$?
  grep -E "^test result" "$OUT/demo-narrow-run.txt"
  grep -E "panicked at|left:|right:" "$OUT/demo-narrow-run.txt" | head -4
  echo "收窄后 --test-threads=8: rc=$rc"
  if [ "$rc" -ne 0 ]; then
    echo "=> 收窄把串行化弄丢了：同一个提交、同样的并行度，测试立刻互相踩。裁决：不能收窄。"
    exit 0
  fi
  echo "=> 意外：收窄后仍然全绿 —— 本轮的『不能收窄』结论不成立，请复核。"
  exit 1
  ;;
*)
  echo "用法: sh qa/harness/m6-demo-f28.sh clippy|serial|parallel|narrowed" >&2
  exit 2
  ;;
esac
