# 合流方案的必答问题（M1–M10）

**这是第 6 阶段写「三棵树合流方案」时的输入清单**：每题都必须给出答案 + 证据（命令与输出），不能只给结论。
本文件本身**不**给出方案——第 1 阶段只负责把问题问准、把素材指针指对。

## 三棵树（事实，来自第 0 阶段与本阶段实测）

| 树 | 位置 | 内容 | 状态 |
|---|---|---|---|
| **A 本地 Windows** | `D:\code\ClusterScope`（只此一份）；素材快照 `ClusterScope-review/local-wip/` | 基线 `f8ac726` + 未提交的 Web 前端工作（2026-09-13）；`version = "0.1.0"`；有 `web/`（27 个文件，Vite+TS）、`tests/integration_test.rs`、以及 `crates/common/src/{dedup,metrics,sequence}.rs` 三个**多出来的模块** | 未提交、未推送 |
| **B node 工作树** | `/public/tianyuyang/code/ClusterScope`（`master` **ahead 12**） | `19d8fbc` + **12 个未提交的修改文件**（2026-08-12）：`Cargo.lock`、`Cargo.toml`、`agent/{job_executor,metrics}.rs`、`common/src/alert.rs`、`server/Cargo.toml`、`server/src/{auth_middleware,grpc,handlers,main,ws_handler}.rs`、`storage/src/user_queries.rs` | 未推送；**bundle 里只有提交，没有这 12 个改动** |
| **C GitHub master** | `ClusterScope-review/gh-line`（HEAD `f9c080b`，本审查对象） | `version = "0.1.1"`；TUI-only（`web/` 已删）、无 `tests/`、无 A 的那三个 common 模块 | 已发布、可构建、有 44 个测试 |

素材与指针：

```sh
# 只读盘点（不需要网络）
cd /public/tianyuyang/code/ClusterScope-review
ls -la node-line.bundle local-wip/                       # B 的 bundle（8.9MB）、A 的原始快照
git -C /public/tianyuyang/code/ClusterScope status --short --branch   # B 的 12 个未提交改动（真身）
git -C /public/tianyuyang/code/ClusterScope log --oneline -5          # B 的 HEAD = 19d8fbc
git -C gh-line log --oneline --graph --decorate --all | head -30      # C 的历史
```

## 必答问题

| # | 问题 | 必须给出的证据（命令） | 验收口径 |
|---|---|---|---|
| M1 | 三棵树的提交图与分叉点到底是什么？谁是超集、谁有独有提交？ | `mkdir -p gauntlet-out/merge && cd gauntlet-out/merge && git init b && cd b && git fetch ../../../node-line.bundle master:refs/heads/bline && git log --oneline bline \| head -20`；`git -C /public/tianyuyang/code/ClusterScope log --oneline --all \| head -20`；`git -C gh-line log --oneline master \| head -20` | 给出三条线的 commit 列表 + 共同祖先（预期 `f8ac726`），并明确指出 **B 的 12 个未提交改动不在任何提交里**，必须单独搬运 |
| M2 | 「谁有什么」的**功能 × 树**矩阵：Web 前端、集成测试、`common/{dedup,metrics,sequence}`、TUI 的 per-core/Top CPU、告警级联删除、requeue 竞态修复…… 各在哪几棵树里存在？ | `diff -rq local-wip/crates gh-line/crates \| head -40`；`ls local-wip/web local-wip/tests`；`ls gh-line/crates/common/src gh-line/crates/tui/src`；`grep -rn 'dedup\|sequence' gh-line/crates/common/src/lib.rs` | 一张矩阵表（行=功能/模块，列=A/B/C，格=有/无/不同实现），每格有文件路径证据 |
| M3 | 被丢弃的工作有哪些、值不值得捡回来？特别是：C 删掉 `web/` 时那些代码还在不在 A/B 里？A 的 `dedup.rs/metrics.rs/sequence.rs` 是 C 里被别的机制替代了，还是纯丢失？ | `ls -la local-wip/web/src`；`git -C gh-line log --diff-filter=D --name-only --oneline \| grep -i web \| head`；`grep -rn 'seen_reports\|LruCache' gh-line/crates/server/src/main.rs`（对照 A 的 `dedup.rs`）；`wc -l local-wip/crates/common/src/{dedup,metrics,sequence}.rs` | 逐项判定：**重复实现 / 被替代 / 真丢失**，并给出「捡回成本」估计 |
| M4 | 冲突点清单：三棵树都改过的文件里，哪些是同一处代码的不同改法（必然冲突）？ | `comm -12 <(cd local-wip && find crates proto deploy docs -type f \| sort) <(cd gh-line && find crates proto deploy docs -type f \| sort) > /tmp/shared.txt; wc -l /tmp/shared.txt`；`git -C /public/tianyuyang/code/ClusterScope diff --stat \| tail -20`；对 B 的 12 个文件逐个 `git -C /public/tianyuyang/code/ClusterScope diff -- <file>` 并与 C 的对应文件比 | 每个冲突点写：文件、双方改法、语义差异、建议取谁（含理由） |
| M5 | 合流的目标形态是哪一个：**(a) TUI-only**（=C 现状，A 的 web 工作不进主线）、**(b) TUI + Web**（要把 A 的 web 接回来）、**(c) 先 TUI-only、web 作为后续分支**？ | 需要**产品决策**：把三者的代价列出来（构建面、依赖、测试面、文档面），每项给出本审查的实测依据 | 这是**必须由人拍板**的题：第 6 阶段给 A/B/C 选项与推荐，不得自行决定 |
| M6 | 推荐路线：以哪棵树为基线？怎么搬？每步的验证命令是什么？ | 必须写出可执行序列，例如「以 C 为基线 → 把 B 的 12 个改动按文件 graft → 把 A 的 web/ 作为独立目录接入 → 每步跑 `cargo build --workspace --all-targets --offline` + `node .gauntlet/gauntlet.mjs test`」 | 每一步都有验证命令与期望输出；禁止「大爆炸式」一次性合并 |
| M7 | 合流后的质量口径怎么办？合流必然引入新代码，而 C 现在已经是 complexity 21 / crap 45 / coverage 20.7% 的 FAIL 状态。 | 引用 `GAUNTLET.md`「硬阈值下的现状」；给出「合流前是否先冻结阈值 / 是否开棘轮 / 是否接受指标继续变差」的选项 | 规则改动**只能由人裁决**（第 0 阶段已裁决：棘轮关、硬阈值判定）；方案里必须写清合流对三个指标的影响方向 |
| M8 | 本审查发现的问题里，哪些**必须在合流时一起修**、哪些可以延后？ | 引用 `constraints.json` 里 `verdict: finding` 的条目（D1 LICENSE、D6 audit-logs 500、DOC-07 天级历史、DOC-12 默认口令、DOC-03 server `--help`、DOC-10/11 死配置键、S12 登录无限速、S13 token 不可吊销、S14 审计覆盖、CON-07 pid 不落库、CON-08 无重试） | 给每项定级：**合流前必修 / 合流后单独修 / 只记录**，并说明判据（是否影响数据正确性、是否影响安全边界、是否影响可运维性） |
| M9 | 合流完成后怎么证明「没丢东西」？ | 列出**行为等价清单**并给出验证命令：REST 端点表（`crates/server/src/main.rs:234-312` vs `docs/api.md`）、TUI 快捷键（`crates/tui/src/main.rs:85-140`）、gRPC 服务方法（`proto/clusterscope.proto`）、DB schema（`crates/storage/src/lib.rs` 的 9 张表）、配置键（`crates/common/src/config.rs`）；命令示例：`node .gauntlet/gauntlet.mjs test`、`sh qa/harness/api-checks.sh`、`sh qa/harness/job-e2e.sh`、`sh qa/harness/doc-claims-checks.sh` | 合流后的仓库能跑通本 QA 包，且 D 类文档检查的 FAIL 数不增加 |
| M10 | **不变量**：合流不得引入新的 root 依赖，系统级 unit 必须修掉或明确区分——B 的 12 个未提交改动 + B 的 12 个独有提交 + A 的独有资产搬进来以后，用户需求「这个项目是要做一个不用 root 的程序」是否仍然成立？怎么证明？ | 逐条回答下面的 `NRM1`–`NRM8`（每条都要贴命令与输出）；再跑全量 `sh qa/harness/no-root-checks.sh`（约束 `NR-01`–`NR-21`） | NRM1–NRM8 全部满足；no-root 检查无**新增** FAIL（既有 FAIL 只允许是 `NR-13`/`NR-15` 两条「系统级 unit + 缺 user unit」，且必须已有处置决定）；`NR-*`/`MRG-02` 在 `qa/qa-report.json` 里逐条配对 |

## M10 的展开：no-root 不变量 `NRM1`–`NRM8`

> 这些不是「可选项」，是把用户需求「这个项目是要做一个不用 root 的程序」变成**合流后必须仍然成立**的判据。
> 每条都要给出命令与实测输出；命令一律在合流后的仓库根目录执行（`R=/public/tianyuyang/code/ClusterScope-review/<合流后的树>`）。

| # | 不变量（合流后必须成立） | 判据 / 验证命令 | 现在的事实（合流前） |
|---|---|---|---|
| NRM1 | **运行时零特权**：server 用普通用户起得来（argv[1] 指用户目录里的 yaml），8080/50051 监听、`/api/health`=200；agent 用普通用户起得来并上报；TUI 在 pty 里渲染 | `id -u`（必须非 0）→ `sh qa/harness/no-root-checks.sh` → `NR1/NR2/NR3/NR18/NR20` 全 PASS | 已成立（本审查实测，uid 3000） |
| NRM2 | **零配置文件也不崩**：server 只靠 `POSTGRES_URL`/`JWT_SECRET`/`AUTH_REQUIRED` 环境变量可启动（`NR-21`）；agent 缺 `-c` 文件时不读取系统路径、日志目录落在 HOME（`NR-06`/`NR-06b`） | `NR21`/`NR6`/`NR6b` 三行 PASS；`lsof -p <server-pid> \| grep -cE '/etc/clusterscope\|/var/(lib\|log)/clusterscope'` 必须为 0 | 已成立（2026-10-07 实测） |
| NRM3 | **产品代码与用户级脚本里 0 处硬编码系统路径**：`crates/**/*.rs` 与 `deploy/install-agent.sh`、`deploy/tui.sh` 中不得出现 `/etc/clusterscope`、`/var/lib/clusterscope`、`/var/log/clusterscope`、`/usr/local/bin`（**允许**：`deploy/*.service` 这类系统级模板、示例 yaml 的注释、README 的说明） | 合流提交上跑：`grep -rn '/etc/clusterscope\|/var/lib/clusterscope\|/var/log/clusterscope\|/usr/local/bin' crates deploy/install-agent.sh deploy/tui.sh \| grep -v '^#'`；允许清单之外的每一处都要有理由 | 合流前：`crates/server/src/main.rs:186`、`crates/agent/src/main.rs:15`、`crates/common/src/config.rs:38` 三处（都是默认值，**均有可覆盖的 CLI/env**）→ 计入「已说明的默认值」，只要求数量不增加 |
| NRM4 | **B 的 12 个未提交文件逐一验证不引入 root 依赖**，重点 `crates/agent/src/{job_executor,metrics}.rs`、`crates/server/src/{main,handlers,grpc,ws_handler}.rs`、`crates/storage/src/user_queries.rs`、`crates/common/src/alert.rs`、两个 Cargo.toml、`Cargo.lock` | `for f in <12 个文件>; do git diff -- "$f" \| grep -nE '/etc/\|/var/lib\|/var/log\|/usr/local\|pre_exec\|setuid\|setgid\|chown\|CAP_'; done` → 命中即 FAIL，必须逐条解释或改回 | 未评估（B 的改动从未被测试/审查过，见 M1/M4） |
| NRM5 | **合流不得新增特权原语**：不出现 `libc::setuid/setgid/setreuid`、`pre_exec` 提权、`chown` 系统路径、`sudo`、`pkexec`；新增依赖里不得引入 `daemonize`/`systemd` crate 之类要求系统级权限的路径 | `grep -rnE 'setuid\|setgid\|pre_exec\|pkexec\|sudo \|chown' crates/`；`git diff <base> --stat -- Cargo.toml crates/*/Cargo.toml` 后逐个新依赖查用途 | 合流前：0 命中（`grep -c` 输出 0） |
| NRM6 | **两套部署件的矛盾必须被修掉或明确区分**（不能继续「README 说 user、仓库发 system」）：要么 (a) 给 server 补一个用户级 unit + README「无 root 快速开始」（零特权启动 server 的文档化步骤：写 yaml → 建 `~/.config/systemd/user/clusterscope-server.service` 或 nohup → `systemctl --user enable --now`），要么 (b) 把 `deploy/*.service` 重命名为 `*.system.service` 并在 README/注释里写明「需要 root，仅为可选项」，同时把 `DOC-21`/`NR-13`/`NR-15` 的结论改为「已区分」 | 合流提交上：`grep -n 'systemctl --user' README.md` 与 `find . -name 'clusterscope-server.service' -not -path './target/*'` **必须同时非空**（选了 (a)），或者 README 里每处 `systemctl --user … clusterscope-server` 都改写/删除并注明系统级前提（选了 (b)）；两种情况都要在 PR 描述里给出选哪条 | 现状：README 3 处 user 管理命令 vs 仓库 0 个 user unit → `NR-13` FAIL；系统级 unit 非 root 装不上 → `NR-15` FAIL |
| NRM7 | **文档承诺与实测口径一致**：M10 的修改若触及 README，`:87` 的 `docker compose up` 承诺必须改成与本集群一致的说明（本机无 docker/docker-compose、无外网，可行路径是「从源码编译 PG 到 HOME」，见 `GAUNTLET.md`「PostgreSQL 16.4」），或明确标注「需要 docker 的节点才成立」 | `grep -n 'docker compose up' README.md`；合流后重跑 `sh qa/harness/doc-claims-checks.sh`，`NR-09`/`NR-10` 两条结论只能变好不能变差 | `NR-09`/`NR-10` 各记 1 条 FAIL（finding） |
| NRM8 | **合流后仍可复现证明**：合流分支上跑 `sh qa/harness/no-root-checks.sh`（全量，含 NR11/NR12 的 sleep 检查）全部 PASS；`node .gauntlet/gauntlet.mjs test` 的 44/44 不减少；本文件 M9 的行为等价清单同时通过 | 三条命令的输出贴在合流 PR 描述里；任何新的 FAIL 都要有对应的 finding 条目（不得静默） | 基线：`tests: 44/44`；no-root 检查 21 条（其中 `NR13`/`NR15` 预期 FAIL） |

**合流完成的判据（一句话）**：在一台**没有 root、没有 docker、没有外网**的节点上，用合流后的代码把 server + agent + TUI 跑起来（`sh qa/harness/no-root-checks.sh` 全 PASS，只允许 `NR-13`/`NR-15` 两条已在 PR 里写明处置的 FAIL），并且没有任何 `crates/**` 代码路径要求写 `/etc`、`/var/lib`、`/var/log`、`/usr/local/bin`。

## 硬约束（写方案时不能违反）

1. **不得丢失 B 的 12 个未提交改动**：它们不在 bundle 里，只在 `/public/tianyuyang/code/ClusterScope` 的工作树里。方案必须包含「先备份这 12 个文件（`git -C … stash` 或复制到 `ClusterScope-review/` 下）」这一步，并给出校验（`sha256sum` 列表）。
2. **不得修改 `gauntlet.config.json` 的 `sources` / `exclude` / `thresholds`**，也不得自行开关棘轮（第 0 阶段已由用户裁决：关）。
3. **A 的 web/ 是否接入，是产品决策**（M5），不能由 agent 自行决定。
4. 方案要能在**离线**环境执行：所有 `cargo` 命令带 `--offline`，不得引入需要联网下载的新依赖；若 web/ 接入需要 npm 依赖，必须写明「需要外网」并交人执行。
5. **合流不得引入新的 root 依赖**（M10 的 `NRM1`–`NRM8`）：这是用户需求级别的不变量，与「不丢东西」（M9）同级；违反即视为合流失败，而不是「后续再修」。
