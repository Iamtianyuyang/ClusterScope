#!/bin/sh
# line-fork-verify.sh -- B(node line) vs C(GitHub line) 的提交图与分叉关系探测。
#
# 背景：第 6 阶段初版把「B 的 HEAD 19d8fbc 是 C 的祖先」写进了报告
#       (report/merge-plan.md / report/_brief.html / GAUNTLET.md)。
#       2026-10-07 返工用本脚本复核，实测为「f8ac726 之后兄弟分叉、互不为祖先」。
#       更正记录见 report/merge-plan.md 顶端「勘误记录」E-1。
#
# 只读探测：只在 /tmp/probe-verify 建一个全新的空仓库，把两个 bundle 取成
#           refs/remotes/b/master 与 refs/remotes/c/master；不写、不动任何工作树，不需要网络。
#
# 运行（测试机 tianyuyang@172.19.133.164；工作目录 = ClusterScope-review/gh-line）：
#   sh qa/evidence/line-fork-verify.sh          # 本脚本的输出即同目录的 line-fork-verify.txt
#
# 复现要点：git 2.47.3 下 `git rev-parse --short b/master c/master`（两个 rev 一起加 --short）
#           会报 "fatal: Needed a single revision"，所以下面拆成两条写。

BUNDLE_DIR=/public/tianyuyang/code/ClusterScope-review
BARE_REPO=/public/tianyuyang/git/ClusterScope.git
PROBE=/tmp/probe-verify

say() { printf '$ %s\n' "$*"; }

echo "# line-fork-verify : B (node line bundle) vs C (GitHub line bundle)"
echo "# git:    $(git --version)"
echo "# host:   $(hostname)"
echo "# date:   $(date -u '+%Y-%m-%dT%H:%M:%SZ') (UTC)"
echo "# probe:  $PROBE (fresh empty repo created below; read-only on every working tree)"
echo "# bundle: $BUNDLE_DIR/node-line.bundle  -> refs/remotes/b/master"
echo "# bundle: $BUNDLE_DIR/gh-line.bundle    -> refs/remotes/c/master"
echo "# bare:   $BARE_REPO (B 的本地裸仓库)"
echo

say "rm -rf $PROBE && mkdir -p $PROBE && cd $PROBE && git init -q ."
rm -rf "$PROBE" && mkdir -p "$PROBE" && cd "$PROBE" && git init -q .
echo

say "git bundle list-heads $BUNDLE_DIR/node-line.bundle"
git bundle list-heads "$BUNDLE_DIR/node-line.bundle"
echo

say "git bundle list-heads $BUNDLE_DIR/gh-line.bundle"
git bundle list-heads "$BUNDLE_DIR/gh-line.bundle"
echo

say "git fetch -q $BUNDLE_DIR/node-line.bundle 'refs/heads/master:refs/remotes/b/master'"
git fetch -q "$BUNDLE_DIR/node-line.bundle" "refs/heads/master:refs/remotes/b/master"
say "git fetch -q $BUNDLE_DIR/gh-line.bundle 'refs/remotes/github/master:refs/remotes/c/master'"
git fetch -q "$BUNDLE_DIR/gh-line.bundle" "refs/remotes/github/master:refs/remotes/c/master"
git for-each-ref --format='%(objectname:short) %(refname)'
echo

say "git rev-parse --short b/master; git rev-parse --short c/master"
git rev-parse --short b/master
git rev-parse --short c/master
echo

say "git merge-base b/master c/master"
git merge-base b/master c/master
echo

say "git merge-base --all b/master c/master   # 唯一共同祖先？（只输出一行即为唯一）"
git merge-base --all b/master c/master
echo

say "git merge-base --is-ancestor b/master c/master; echo \$?   # 0 = b 是 c 的祖先"
git merge-base --is-ancestor b/master c/master
echo "$?"
echo

say "git merge-base --is-ancestor c/master b/master; echo \$?   # 0 = c 是 b 的祖先"
git merge-base --is-ancestor c/master b/master
echo "$?"
echo

say "git rev-list --count c/master..b/master   # B 独有"
git rev-list --count c/master..b/master
echo

say "git rev-list --count b/master..c/master   # C 独有"
git rev-list --count b/master..c/master
echo

say "git rev-list --count b/master; git rev-list --count c/master   # 总提交数"
git rev-list --count b/master
git rev-list --count c/master
echo

say "git rev-list --count f8ac726   # 共同前缀的提交数"
git rev-list --count f8ac726
echo

say "git log --oneline --no-decorate c/master..b/master   # B 独有 15 个"
git log --oneline --no-decorate c/master..b/master
echo

say "git log --oneline --no-decorate b/master..c/master   # C 独有 33 个"
git log --oneline --no-decorate b/master..c/master
echo

say "git rev-parse --short 09f7460^; git rev-parse --short 5f210c2^   # 分叉后各自的第一个提交"
git rev-parse --short 09f7460^
git rev-parse --short 5f210c2^
echo

say "git diff --stat f266f2f 75c3f98   # 空 = B/C 第一个提交 tree 相同（同 parent，仅提交者/时间戳不同）"
git diff --stat f266f2f 75c3f98
echo "diff exit=$?"
echo

say "git diff --stat 15b47a7 963ed9c; git diff --stat d1586b6 d43af1f"
git diff --stat 15b47a7 963ed9c; echo "diff exit=$?"
git diff --stat d1586b6 d43af1f; echo "diff exit=$?"
echo

say "git patch-id --stable  (f266f2f / 75c3f98)   # 相同 = 同一份改动"
git show f266f2f | git patch-id --stable | cut -d' ' -f1
git show 75c3f98 | git patch-id --stable | cut -d' ' -f1
echo

say "git -C $BARE_REPO rev-parse --short master   # B 的本地裸仓库停在 d1586b6"
git -C "$BARE_REPO" rev-parse --short master
say "git rev-list --count d1586b6..b/master   # 裸仓库落后 B 的提交数 = 12"
git rev-list --count d1586b6..b/master
echo

echo "# 结论：b/master 与 c/master 互不为祖先（两次 is-ancestor 都是 exit 1），"
echo "#       共同祖先 f8ac726 唯一；B 独有 15 个提交、C 独有 33 个提交。"
