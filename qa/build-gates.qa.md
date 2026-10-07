# QA：真实构建 / 测试 / 静态闸门（G1–G12）

审查对象：`f9c080b`（本仓库工作树）。**这些数字第 0 阶段已实测**，第 5 阶段只需**复现并记录**，不需要重新摸底。

前置：

```sh
cd /public/tianyuyang/code/ClusterScope-review/gh-line
export PATH=$HOME/.cargo/bin:$PATH          # cargo 不在默认 PATH
# 远端无外网：所有 cargo 命令必须带 --offline；不要跑 cargo fetch / cargo metadata
```

| # | 检查项 | 操作（确切命令） | 期望结果 | 判定 | 证实约束 | 证据 |
|---|---|---|---|---|---|---|
| G1 | 构建闸门 | `node .gauntlet/gauntlet.mjs gate --profile quality 2>&1 \| tee gauntlet-out/qa/evidence/gate.txt \| grep -E '^(build\|GATE)'` | `build ✅ PASS`；最后一行 `GATE quality: FAIL` | 构建 PASS 即算通过；`GATE quality` 必须是 FAIL（本审查不修复杂度/CRAP/覆盖率） | GATE-01 | `gauntlet-out/qa/evidence/gate.txt`、`gauntlet-out/gate.json` |
| G2 | 测试闸门 | `node .gauntlet/gauntlet.mjs test 2>&1 \| tee gauntlet-out/qa/evidence/test.txt \| tail -5` | `tests: 44/44 passed`，0 失败 0 忽略 | 退出码 1 **只允许**来自 `ACCEPTANCE scenarios=0`（features/ 按裁决为空） | GATE-02、GATE-10 | 同上；`gauntlet-out/junit.xml` |
| G3 | clippy + rustfmt | `sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo clippy --workspace --all-targets --offline --message-format=short; echo clippy=$?; cargo fmt --all --check; echo fmt=$?'` | `clippy=0`、`fmt=0`，无 warning 行 | 两个退出码都是 0 | GATE-03 | `gauntlet-out/qa/evidence/lint.txt` |
| G4 | 重复代码 + 测量范围 | `node .gauntlet/gauntlet.mjs gate --profile quality 2>&1 \| grep -E 'duplication\|scope'` | `duplication ✅ 0.0%`、`scope ✅ 33/33` | 两行都是 ✅ | GATE-04 | `gauntlet-out/duplication.json`、`static.json` |
| G5 | 复杂度闸门（硬阈值） | 同 G1，`grep '^complexity'` | `❌ FAIL`，316 函数中 21 个超标，maxCC=23 | **FAIL 是审查结论**（`crates/tui/src/ui.rs:599 node_panel`） | GATE-05 | `gauntlet-out/static.json`、`next.md` |
| G6 | CRAP 闸门（硬阈值） | 同 G1，`grep '^crap'` | `❌ FAIL`，45 函数超标，maxCRAP=552 | **FAIL 是审查结论**（44/45 覆盖率 0%） | GATE-06 | `gauntlet-out/crap.json` |
| G7 | 覆盖率闸门（硬阈值） | 同 G1，`grep '^coverage'` | `❌ FAIL`，20.7%（1381/6686），阈值 90% | **FAIL 是审查结论**；另记 crate 分布：storage 0.0% / server 7.5% / tui 4.0% | GATE-07 | `gauntlet-out/coverage.lines.json` |
| G8 | 架构闸门 | 同 G1，`grep '^arch'` | `➖ skipped`（未配 `commands.arch`） | 跳过即通过本检查，但**必须写进报告**为「无自动架构检查」 | GATE-08 | `gauntlet-out/gate.json` |
| G9 | 无 CI | `test -d .github && echo CI-PRESENT \|\| echo CI-ABSENT; git log --oneline \| wc -l` | `CI-ABSENT`；提交数 40（首提交 2026-08-10，HEAD 2026-08-14） | CI-ABSENT → 报告里记为「三条闸门只能人工手跑」 | GATE-09 | `gauntlet-out/qa/evidence/no-ci.txt` |
| G10 | `test` 退出码归因 | 见 G2 输出里的 `ACCEPTANCE` 行 | `ACCEPTANCE scenarios=0 … FAIL` | 确认退出码 1 的原因**只有** features/ 为空；不得因此判构建/测试坏 | GATE-02、GATE-10 | `gauntlet-out/qa/evidence/test.txt` |
| G11 | release 交付物 | `ls -l target/release/clusterscope-{agent,server,tui}` | 三个文件存在（约 5.9M / 12.4M / 8.1M） | 三个都在 → PASS | GATE-11 | `gauntlet-out/qa/evidence/release-bin.txt` |
| G12 | 覆盖率机制可用 | `node .gauntlet/gauntlet.mjs test >/dev/null 2>&1; ls -l gauntlet-out/junit.xml gauntlet-out/lcov.info; head -3 gauntlet-out/lcov.info` | 两个报告都在，lcov 首行是 `SF:` | 报告存在即 PASS（证明无 cargo-llvm-cov 也有覆盖率） | GATE-12 | `gauntlet-out/lcov.info` |

## 复现要点（踩过的坑）

- **不要 `| head`**：kit 命令被 SIGPIPE 打断会拿到陈旧报告（GAUNTLET.md 坑 #7）。
- **不要并发跑两条 kit 命令**：`test` 每次开始会删 `junit.xml` / `lcov.info`。
- 插桩构建用独立 `CARGO_TARGET_DIR=gauntlet-out/cov-target`，与 `target/` 互不污染。
- 失败清单与「距离」见 `gauntlet-out/next.md` 与 `GAUNTLET.md`「硬阈值下的现状」，本阶段不重跑 `next`。
