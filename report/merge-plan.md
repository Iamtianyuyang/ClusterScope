# 三棵树合流方案（第 6 阶段交付 · M1–M10 逐题）

> 审查对象：**C = GitHub 已发布线** `f9c080b`（TUI-only，40 个提交，`version 0.1.1`）。
> 本文件回答 `qa/merge-plan-requirements.md` 的 M1–M10，每题给**事实**（带命令与证据）与**建议**（可执行、代价可估）。
> 事实来自第 5 阶段已核验的 `qa/evidence/merge-plan-facts.txt`，以及本阶段在父目录 `ClusterScope-review/` 的
> `node-line.bundle` / `gh-line.bundle` / `local-wip/` 上重新跑的探测（`gauntlet-out/merge/*.txt`）。
> **`M5` 是产品决策，本文件只给选项与代价，不替人拍板。**
>
> **2026-10-07 增补（本文件第 2 版）**：并入 **no-root 维度的合流不变量 `M10`（`NRM1`–`NRM8`）**，
> 把第 5 阶段增补发现的 **NF-01 / NF-02 升级为「合流前必修」**，并在 M6 的路线里加入两项合流后待办
> （补用户级 unit、文档化无 root 的数据库路径）。**上一轮的勘误记录 E-1/E-2 原样保留**，M1–M9 的实测数字一个未改。

三棵树的代号沿用 `qa/merge-plan-requirements.md`：

| 代号 | 位置 | HEAD | 状态 |
|---|---|---|---|
| **A** | 本地 Windows `D:\code\ClusterScope`；素材快照 `ClusterScope-review/local-wip/` | `f8ac726` + 未提交 Web 工作 | 只此一份，未提交、未推送 |
| **B** | node `/public/tianyuyang/code/ClusterScope`（裸仓库 `/public/tianyuyang/git/ClusterScope.git`） | `19d8fbc` | 22 个提交；12 个文件未提交；从未推送到 GitHub |
| **C** | `ClusterScope-review/gh-line` = GitHub `master` | `f9c080b` | 已发布、可构建、44 个测试 |

---

## 勘误记录（2026-10-07，第 6 阶段返工）

> 审查报告的价值来自可追溯：这里**保留初版是怎么写错的**、以及更正后的实测事实，不静默改掉。
> 本次返工只动"M1 的结构判断"与"M6 步骤 0 的状态"，**不动** 16 条 findings、闸门面板、无法验证项与 N1–N9 的任何实测数字，也不改变产品状态。

| # | 初版写法 | 实测（复现命令见下） | 对方案的影响 |
|---|---|---|---|
| **E-1** | 「**B 的 HEAD `19d8fbc` 是 C 的祖先**（`git merge-base --is-ancestor b/master c/master` → YES）」，并据此推论「C = B 的全部提交历史 **+ 33 个新提交**」「B 相对 GitHub 没有"分叉"，是**落后**」「三线 = 一条主干 + 一棵离线工作树」 | `git merge-base --is-ancestor b/master c/master` → **exit 1**；反向 `git merge-base --is-ancestor c/master b/master` → **exit 1**。**B 与 C 自 `f8ac726` 兄弟分叉、互不为祖先**：B 独有 **15** 个提交、C 独有 **33** 个提交（`git rev-list --count`） | **改变了合流的结构判断**：合流不是"把落后的 B 拉进 C"，而是**三件事**——(1) B 工作树的 12 个未提交文件；(2) B 那 12 个独有提交里修法的取舍（与 C 同主题提交修法不同）；(3) A 的独有资产（按 M5 裁决）。M6 的「不要整体 `git merge 19d8fbc`（35 个冲突文件）」**这条建议不变、仍然成立** |
| **E-2** | M6 步骤 0 写成「**先做，不可跳**」（只给命令，未说明产物是否留存） | 该步骤**已实际执行**、产物已留档在 `ClusterScope-review/backup-b-wip/`：`b-wip.tar.gz`（**12 个成员**，sha256 `8ef25e55af7cb612162cfc9887fd88dd52dba55260aa1c7508f984756fd9b4df`）+ `list.txt` + `sha256-manifest.txt` + `PROVENANCE.txt`。**同输入重建得到同一哈希**——本次返工又独立重建一次，`cmp` 逐字节相同 | 步骤 0 的性质从"待做的前置动作"改为"**已完成、只需引用**"（见 M6 步骤 0） |

**E-1 的复现命令**（在测试机 `tianyuyang@172.19.133.164` 上，`/tmp` 里的**全新**探测仓库；完整脚本与原始输出见 `qa/evidence/line-fork-verify.sh` 与 `qa/evidence/line-fork-verify.txt`）：

```sh
cd /public/tianyuyang/code/ClusterScope-review
rm -rf /tmp/probe-verify && mkdir -p /tmp/probe-verify && cd /tmp/probe-verify && git init -q .
git fetch -q /public/tianyuyang/code/ClusterScope-review/node-line.bundle "refs/heads/master:refs/remotes/b/master"
git fetch -q /public/tianyuyang/code/ClusterScope-review/gh-line.bundle "refs/remotes/github/master:refs/remotes/c/master"
git rev-parse --short b/master; git rev-parse --short c/master     # → 19d8fbc / f9c080b
git merge-base b/master c/master                                   # → f8ac7267f834a2b65da1aaf9c065f2de3c524250
git merge-base --is-ancestor b/master c/master; echo $?            # → 1  ← 不是祖先（初版误记为 YES）
git merge-base --is-ancestor c/master b/master; echo $?            # → 1  ← 反向也不是
git rev-list --count c/master..b/master                            # → 15（B 独有）
git rev-list --count b/master..c/master                            # → 33（C 独有）
```

> **错因**：附录里那条 `git merge-base --is-ancestor b/master c/master && echo YES` 在"不是祖先"时**什么都不打印**（`&&` 短路），
> 空输出被误读成了 YES。附录与 `qa/evidence/line-fork-verify.txt` 已改成显式 `echo $?` 的写法。
> **环境**：测试机 `git 2.47.3`；该版本下 `git rev-parse --short b/master c/master`（两个 rev + `--short`）会报 `fatal: Needed a single revision`，
> 所以上面拆成两条写（哈希与结论不受影响）。

---

## 增补记录（2026-10-07，no-root 维度并入）

> 增补原因：用户需求原文是「**这个项目是要做一个不用 root 的程序**」。上一轮的审查只把「无 root」当成一条文档不符
> （`DOC-21`）记录。第 1 阶段增补把它拆成 **23 条可判定约束**（`NR-01`…`NR-21`、`NR-06b`、`MRG-02`）并在
> `qa/merge-plan-requirements.md` 里加了 **M10 + 不变量 `NRM1`–`NRM8`**；第 5 阶段增补在真实 release 二进制上逐条复跑、
> 独立复推，并把 **2 条新 finding（NF-01 / NF-02）** 写进 `qa/qa-report.json`。本节只记录**这些新事实对合流方案的影响**。

| # | 增补事实 | 对合流方案的影响 |
|---|---|---|
| **A-1** | **运行时确实不需要 root**：uid 3000、`CapEff=0000000000000000`、零 HOME 外写入，server（8080/50051 监听、`/api/health`=200）、agent、TUI（pty 正常渲染）全功能可用；端口都 >1024 不需要 `CAP_NET_BIND_SERVICE`；`/proc` 权限不足时按 README 承诺降级 | `NRM1` **成立** → 合流**不得引入**任何特权原语（`NRM4`/`NRM5` 就是这条的判据） |
| **A-2** | **NF-01（major）**：agent 在「干净 HOME + `-c` 指向缺失文件」下 **exit 1**，报错只说 `Failed to write node identity`、**从不提配置文件缺失**；配置缺失时还会**静默回退**（config 写 59999、实际拨 `http://localhost:50051`） | **合流前必修**（Leader 裁决 1）：至少做到「缺配置时明确报错 + 创建父目录」。它直接打在 `NRM2` 上 |
| **A-3** | **NF-02（major）**：`deploy/install-agent.sh:80` 的 nohup 分支先跑 `pkill -f clusterscope-agent`，**会杀掉同用户所有 agent**（含它没启动的；本机常驻 agent PID 266643 即活体受害者）。第 5 阶段未执行它（取证来自源码 + 实时进程表） | **合流前必修**（Leader 裁决 1）：1 行改动——不要 `pkill -f`，改成按 PID / 精确匹配 |
| **A-4** | **仓库 0 个用户级 server unit**：README:288-289 的 `systemctl --user restart clusterscope-server` 在**干净机器**上必然失败（本机那份 `~/.config/systemd/user/clusterscope-server.service` 是手写私货，2026-08-10，不属于仓库）；随仓库发的是系统级 `deploy/*.service`，非 root 实测装不上（`systemctl link` → `Interactive authentication required`；`cp` → `Permission denied`） | `NRM6` **不成立** → 写进 M6 的**合流后待办**（Leader 裁决 2：本次审查不改产品代码） |
| **A-5** | **无 root 的数据库路径未文档化**：README:57/87 承诺「无 root 时可用 `docker compose up`」，但本机无 docker、podman 零镜像且无外网 → 该承诺在本环境不可执行；真正可行的路径（源码编译 PG 到 HOME）README **0 处**记载 | `NRM7` **不成立** → 写进 M6 的**合流后待办** |
| **A-6** | **linger 是每机配置**：本机 `Linger=yes`（`loginctl show-user tianyuyang` → `Linger=yes`、`State=active`），用户级服务活过登出**本机实测 ✅**；但 `linger=no` 的节点上用户级 agent 会**随登出而死**，而 `loginctl enable-linger` 通常需要管理员/root | 写成**部署前提**（见 M10「部署前提」），不替集群做假设；`linger=no` 分支本机**无法复现** |
| **A-7** | **诚实项（清单自身的不精确，作为 discovery D-N14 并入）**：`qa/merge-plan-requirements.md` 里 `NRM5` 的 `grep -E "setuid\|setgid\|pre_exec\|pkexec\|sudo \|chown"` 在 GNU grep 下**匹配空集**（ERE 里 `\|` 是字面竖线），它的「0 命中」是**模式假象**；用正确的 ERE 复跑命中 2 行（见 M10 的 NRM5） | 结论不变（仍无特权原语），但**不能**再拿那条命令当证据；M10 里已换成正确模式 |
| **A-8** | **作者 harness 的 NR6 PASS 是假阳性**：`qa/harness/no-root-checks.sh:171-177` 用 `grep -q 'ClusterScope Agent starting'` 判 NR6、**从不读退出码**，而问候行在崩溃前就打印了 | `NR-06` 的最终判定**以第 5 阶段为准**（`Q305`：`violated`，见 `qa/qa-report.json`）；`NRM2` 的「不崩」前半句**不成立** |

> 增补**没有**改动任何产品代码 / 测试 / `deploy/` / `README.md` / `docs/`；也没有改动既有 81 条约束与第 5 阶段写入
> `qa/qa-report.json` 的任何 verdict。M6/M8 的推荐路线与定级判据仍是上一轮那套，这里是**追加**。

---

## M1 提交图与分叉点

**事实**（命令与输出）

```sh
# 在 /public/tianyuyang/code/ClusterScope-review 下建探测仓库，把两个 bundle 都取进来
git bundle list-heads node-line.bundle   # → 19d8fbc  refs/heads/master
git bundle list-heads gh-line.bundle     # → f9c080b  refs/remotes/github/master
```

| 问题 | 事实 |
|---|---|
| B 的提交数 | 22（`git rev-list --count b/master`） |
| C 的提交数 | 40（`git rev-list --count c/master`） |
| 共同祖先 | `f8ac726` —— **`git merge-base b/master c/master` 的输出就是这个哈希**，也就是 A 的基线提交 |
| B 独有的提交 | **15 个**（`git log --oneline c/master..b/master`） |
| C 独有的提交 | **33 个**（`git log --oneline b/master..c/master`） |
| 关键结构事实 | **B 与 C 在 `f8ac726` 之后兄弟分叉**：`git merge-base --is-ancestor b/master c/master` → **exit 1**，反向 `git merge-base --is-ancestor c/master b/master` → **exit 1**。两线**互不为祖先**，各有独有提交（B 15 / C 33）——不存在"C 包含 B"或"B 只是落后"这回事（初版在这里判错，见上面的勘误记录 E-1）。 |
| B 的 12 个未提交改动 | 不在任何提交里、也不在 bundle 里；只存在于 `/public/tianyuyang/code/ClusterScope` 的工作树（`git status --short` 12 个 `M` 行，与清单逐字一致） |

分叉点之后两条线各自推进，但**前 3 个提交是同一份改动的两次提交**：B 的 `f266f2f` / `15b47a7` / `d1586b6` 与 C 的 `75c3f98` / `963ed9c` / `d43af1f` 逐位对应，
**父提交相同（第 1 对都是 `f8ac726`，后两对各自指向前一对"孪生"提交）、tree 相同**（`git diff --stat f266f2f 75c3f98` 为空；三对的稳定 patch-id 逐对相同），只有 author/committer 与时间戳不同，因此哈希不同。
从第 4 个提交起分道扬镳：B 继续做 **12 个**提交（`09f7460` … `19d8fbc`）、C 继续做 **30 个**（`5f210c2` … `f9c080b`，`git rev-parse 09f7460^`= `d1586b6`、`git rev-parse 5f210c2^`= `d43af1f`），**两边的这 12 / 30 个提交互不包含**。
B 那 12 个提交是 B 的**独有修复**，其中相当一部分在 C 里被独立地重新实现过（例如 C 的 `f4a8a31`、`57938b4`、`0b87b0d`、`5f210c2`），但**修法与覆盖范围不同**（见 M8）——这正是"两条线各修一遍、谁也没进谁"的直接原因。

**结论**：三条线 = **一棵离线的工作树（A）＋ 两条真正的兄弟分支（B、C）**。共同前缀是 7 个提交（`f46b6a9` … `f8ac726`，也就是 A 的本地 `master`）；
`f8ac726` 之后 B 与 C **互不为祖先**：B 独有 15 个提交 = 3 个与 C 内容相同的重复提交 **+ 12 个 B 独有修复**，
C 独有 33 个提交 = 同样的 3 个 **+ 30 个 C 独有提交**。
所以**合流要处理的是三件事**，不是一件事：(1) B 工作树里那 12 个未提交文件；(2) B 那 12 个独有提交里的修法如何与 C 的同主题提交取舍（见 M8）；
(3) A 的独有资产（`web/`、`dedup.rs`/`sequence.rs`/`metrics.rs`、`conversions.rs`、`integration_test.rs`、`nginx.conf`，按 M5 裁决）。

"B 领先裸仓库 12 且从未推送"指的是 **B 的本地裸仓库 `/public/tianyuyang/git/ClusterScope.git`**（HEAD 停在 `d1586b6`，即 B 的 HEAD 往回数 12 个提交：`git rev-list --count d1586b6..b/master` → 12），
**不是** GitHub —— 相对 GitHub，B 既不是"领先"也不是"落后"，而是**分叉**：GitHub 有 33 个 B 没有的提交，B 有 15 个 GitHub 没有的提交。

---

## M2 功能 × 树 矩阵

| 功能 / 模块 | A（local-wip） | B（19d8fbc + wip） | C（f9c080b） | 证据 |
|---|---|---|---|---|
| `web/` 前端（Vite+TS+React） | **27 个文件**（含 `src/auth.ts`、`src/i18n.tsx`、`smartisan.css`、`NodesPage.css`） | 20 个文件（已跟踪） | **0**（`6784ee6 refactor: remove web UI` 删除） | `find local-wip/web -type f \| wc -l`=27；`git log --diff-filter=D --name-only b/master..c/master -- 'web/*'` |
| `tests/integration_test.rs` | 有（179 行，10 个用例） | 有（已跟踪） | **无** | `git ls-tree -r --name-only c/master \| grep tests/` → 空 |
| `crates/common/src/dedup.rs` | **有**（95 行） | 无 | 无 | A 独有 |
| `crates/common/src/sequence.rs` | **有**（65 行） | 无 | 无 | A 独有 |
| `crates/common/src/metrics.rs` | 有（190 行） | **有**（已跟踪，与 A 逐字相同） | **无** | `diff` 头部一致；C 的 `crates/common/src/` 没有 metrics.rs |
| `crates/storage/src/conversions.rs` | 有（26 行） | 无 | 无 | A 独有 |
| `deploy/nginx.conf` | **有** | 有 | 无（`deploy/` 只有 compose + Dockerfile） | `ls` 两侧对比 |
| `deploy/{docker-compose.yml,Dockerfile.server}` | 无 | 有 | **有** | C 独有 |
| TUI per-core 条带 / Top CPU 进程 | 无 | 有 | **有**（C 的 `ui.rs` 49185 字节 vs A 的 28314） | `9f01f87 feat: TUI CPU monitoring` |
| 告警规则删除级联 | 无 | 有 | 有（`fba6661`） | C 的 `alert.rs` 有 `remove_rule`，B 有 `remove_rule_instances` |
| 登录 IP 限速 / WS 每 IP 连接数上限 / 最后管理员保护 / refresh 令牌批量吊销 | 无 | **有**（未提交） | **无** | `rg 'limiter\|ip_conn_count\|delete_user_guarded\|revoke_all_refresh_tokens'`：B-wip 18 处命中，C 0 处 |
| 任务参数上限（`MAX_ARGS`/`MAX_ARG_LEN`） | 无 | **有**（未提交） | **无** | B-wip `handlers.rs:24,25,579,580`；C 无 |
| SIGTERM→SIGKILL 升级 | 无 | **有**（未提交） | **无**（C 的 `SIGKILL` 命中 0） | B-wip `job_executor.rs:107,755,847` |
| 审计覆盖（13 个写入点） | 无 | **有**（未提交） | **无**（只有 2 个：create_job / stop_job） | B-wip `handlers.rs` 13 处 `insert_audit_log` |
| 数据库 | **SQLite**（`sqlx` 带 `sqlite` feature） | PostgreSQL | **PostgreSQL 16** | 三份 `Cargo.toml` 的 `sqlx` feature 行 |
| 版本号 | `0.1.0` | `0.1.1` | `0.1.1` | `Cargo.toml:14` |

---

## M3 被丢弃的工作：重复实现 / 被替代 / 真丢失

| 项 | 判定 | 理由与证据 | 捡回成本 |
|---|---|---|---|
| A `dedup.rs`（`SequenceDeduplicator` + `SequenceTracker`） | **被替代** | C 用 `crates/server/src/main.rs:33,78-80` 的 `seen_reports: LruCache<String, ()>`（10 万条有界）做同一件事，`grpc.rs:181` 用 `format!("{}:{}", node_id, sequence)` 当 key——语义等价 | 0（不捡） |
| A `sequence.rs`（`SequenceGenerator` / `AtomicCounter`） | **被替代** | C 的 `crates/agent/src/grpc_client.rs:30-34` 用 `AtomicU64` 从 wall-clock 毫秒播种，比 A 的每节点 `HashMap` 更简单且重启后不倒退 | 0（不捡） |
| A `metrics.rs`（190 行指标类型） | **真丢失（对 C 而言）** | B 有同名同内容文件；C 的 `crates/common/src/` 与整个 workspace 都没有这些类型 | 低（`cp` 回来 + 在 `lib.rs` 加 `pub mod metrics;`，需确认无重名冲突） |
| A `storage/conversions.rs`（26 行 proto↔row 转换） | **真丢失** | C 的 `crates/storage/src/` 无对应文件 | 低 |
| A `tests/integration_test.rs`（179 行，10 个用例） | **真丢失** | C 无 `tests/` 目录（`tests/` 是 A/B 的独有资产） | 低，但**用例是按旧 API 写的**，合流后需改签名 |
| A `deploy/nginx.conf` | **真丢失** | C 的 `deploy/` 只有 compose/Dockerfile（C 把 nginx 配置写进了 `ca97f1f` 的文档） | 低 |
| A `web/`（27 文件） | **C 主动删除** | `6784ee6 refactor: remove web UI — TUI-only monitoring`；C 的 README 明确写"Web 前端已移除" | **高**（见 M5） |

---

## M4 文件级冲突清单

### A ↔ C（同名同路径 49 个，几乎全部不同）

`diff -rq local-wip/crates gh-line/crates` 报 **39 个同名文件不同 + 4 个 A 独有**。冲突根源只有两条：

| 冲突源 | 具体文件 | 双方改法 | 建议 |
|---|---|---|---|
| **① 数据库后端从 SQLite 换成 PostgreSQL** | 根 `Cargo.toml`（`sqlx` feature：A `sqlite` vs C `postgres`）、`crates/storage/**` 全部 8 个文件、`crates/server/src/**` | A 用 SQLite 语义（`?` 占位符、`sqlite` 类型），C 用 `pg` 占位符 `$n`、`PgPool` | **取 C**。A 的 storage/server 是旧后端，任何逐行合并都会产生语义错误 |
| **② A 的依赖面比 C 宽** | 根 `Cargo.toml` 的 `[workspace.dependencies]`：A 多 29 行（`hyper`/`dashmap`/`rcgen`/`rustls`/`notify`/`globset`/`fs4`/`colored`/`indexmap`/`regex`/`itertools`/`once_cell`/`config`/`tracing-appender`/`serde_with`/`serde-reflection`/`sha2`/`rand`/`async-trait`/`futures`/`bytes`/`home`/`tokio-util`/`tower`/`thiserror`…） | C 在 `4211c2d refactor: drop unused agent dependencies`、`47e4e03 refactor: remove dead code, unused dependencies` 里**主动删掉**了这些 | **取 C**，只在确实需要某个 crate 时逐个加回 |

`deploy/` 与 `docs/`：

| 文件 | A vs C | 建议 |
|---|---|---|
| `deploy/install-agent.sh` | **完全相同**（3183 字节，`cmp` 通过） | 无冲突 |
| `deploy/{agent.service,server.service,agent.yaml.example,server.yaml.example,tui.sh}` | 5 个都不同（差 12~43 字节） | 取 C（C 版本对齐了 `--config` 参数与 systemd 系统级 unit，见 QA OPS-03） |
| `deploy/nginx.conf` | 只有 A 有 | 若保留 Web，捡回并对照 C 的 `ca97f1f` 文档校准 |
| `docs/{api.md,architecture.md}` | 两个都不同 | 取 C，再按合流结果补文档 |

### B（12 个未提交文件）↔ C

C 的 33 个独有提交对这批文件的改动密度（`git log --oneline f8ac726..c/master -- <file>` 计数）：

| 文件 | C 侧提交数 | B 侧提交数 | 冲突性质 |
|---|---|---|---|
| `crates/server/src/main.rs` | **9** | 9 | 状态结构 + 路由 + 后台循环，最热 |
| `crates/server/src/handlers.rs` | **8** | 10 | 每个 handler 都被双方重写过 |
| `crates/agent/src/job_executor.rs` | 5 | 9 | B 有 SIGKILL 升级与重试；C 有 orphan 回收与 marker 重放 |
| `crates/server/src/grpc.rs` | 6 | 9 | B 有任务参数上限与审计；C 有去重 key 与 status 映射 |
| `crates/agent/src/metrics.rs` | 7 | 3 | C 侧改成 NVML + per-core；B 侧是同方向的另一版 |
| `crates/server/src/ws_handler.rs` | 3 | 5 | B 有每 IP 连接数上限 |
| `crates/common/src/alert.rs` | 2 | 3 | B 有 `remove_rule_instances`；C 有 `remove_rule`（`fba6661`） |
| `crates/storage/src/user_queries.rs` | **1** | 4 | C 几乎没动 → B 的 `*_guarded` / `revoke_all_refresh_tokens` **冲突面最小、价值最高** |
| `crates/server/src/auth_middleware.rs` | **1** | 3 | 同上，冲突面小 |
| `Cargo.toml` / `Cargo.lock` / `crates/server/Cargo.toml` | 2 / 2 / 1 | 1 / 3 / 1 | 版本与依赖清单 |

**实测**：把 B 的 `eac070e`（修 F-01 + F-16 的那个提交）直接 3-way merge 到 C，**整体失败**——35 个文件冲突，其中包含 `modify/delete`（`web/*`、`tests/integration_test.rs`、`deploy/nginx.conf` 在 C 被删、在 B 被改）。
但**只 cherry-pick 它改动的 5 个文件**时，冲突收敛到 **4 个文件、6 个冲突块、约 140 行**：

```
crates/storage/src/audit_queries.rs   2 个冲突块（占位符下标 + COUNT 绑定）
crates/storage/src/job_queries.rs     1 个冲突块（32 行）
crates/server/src/handlers.rs         2 个冲突块（81 行）
crates/server/src/main.rs             1 个冲突块（13 行）
crates/storage/src/models.rs          0 个冲突块（自动合并成功）
```

---

## M5 合流的目标形态（**需产品决策**，本文件不拍板）

| 选项 | 代价 | 本审查的实测依据 |
|---|---|---|
| **(a) TUI-only**（= C 现状，A 的 web 工作不进主线） | 最小。只需处理 B 的 12 个未提交文件 | C 已可构建、44 测试通过；`web/` 的删除是 C 的明确决策（`6784ee6`） |
| **(b) TUI + Web**（把 A 的 web 接回来） | **需要外网**（`npm install`，本机离线）；A 的 web 按 **SQLite 时代**的 API 写（`/users` 在 read-only 下无 token 返回 200，C 是 401），接回前要逐端点对齐；C 删掉 web 时连 `deploy/nginx.conf` 一起删了，部署面也要补 | A 的 `web/src/services/api.ts` 只用 10 个路径（`/login` `/refresh-token` `/nodes` `/metrics/history` `/jobs` `/alerts/rules` `/alerts/events` `/cluster/info` `/users`），**全部在 C 的路由表里存在**（`crates/server/src/main.rs:235-291`）→ 契约大体兼容，差异集中在 read-only 鉴权边界与响应形状 |
| **(c) 先 TUI-only、web 作为后续分支**（推荐给用户考虑） | 主线先拿到 B 的修复；web 在独立分支上按 C 的 API 契约移植，验收后再合 | 同上；且本审查发现 C 的 `GET /api/audit-logs` 恒 500（F-01）——web 的审计页在 C 上本来也是坏的 |

---

## M6 推荐路线（可执行序列）

**基线取 C**（已发布、可构建、44 测试通过），分 7 步（0–6，含 no-root 的合流前必修与合流后待办），每步都有验证命令：

### 步骤 0 — 冻结 B 的 12 个未提交文件（**已执行，产物已留档**）

**状态：2026-10-07 已实际执行。**（本步骤初版写作"先做、不可跳"；产物现在已经在盘上，合流时**只需引用**，不要去动这个目录。）

| 产物 | 路径 | 内容 |
|---|---|---|
| 归档 | `ClusterScope-review/backup-b-wip/b-wip.tar.gz` | **12 个成员**；`sha256sum` = `8ef25e55af7cb612162cfc9887fd88dd52dba55260aa1c7508f984756fd9b4df` |
| 清单 | `ClusterScope-review/backup-b-wip/list.txt` | 12 个仓库相对路径（逐行） |
| 逐文件哈希 | `ClusterScope-review/backup-b-wip/sha256-manifest.txt` | 12 个文件的 sha256 |
| 来源 | `ClusterScope-review/backup-b-wip/PROVENANCE.txt` | 源工作树 `/public/tianyuyang/code/ClusterScope`、HEAD `19d8fbcf5a26b8b8247ccea8b7b3205a58b12c7a`、branch `master`、采集时间 `2026-10-07T06:35:11+08:00`、files: 12 |

**同输入重建可得同一哈希（已复现）**——验证方式（只读 B 的工作树，产物写 `/tmp`）：

```sh
rm -rf /tmp/rebuild-b-wip && mkdir -p /tmp/rebuild-b-wip && cd /tmp/rebuild-b-wip
git -C /public/tianyuyang/code/ClusterScope status --short | awk '{print $2}' > list.txt
tar czf b-wip.tar.gz -C /public/tianyuyang/code/ClusterScope -T list.txt
sha256sum b-wip.tar.gz                  # → 8ef25e55af7cb612162cfc9887fd88dd52dba55260aa1c7508f984756fd9b4df
tar tzf b-wip.tar.gz | wc -l            # → 12
cmp b-wip.tar.gz /public/tianyuyang/code/ClusterScope-review/backup-b-wip/b-wip.tar.gz && echo identical
```

校验：成员数 **12**；12 个文件的 sha256 见 `backup-b-wip/sha256-manifest.txt`（与本报告附录 `qa/evidence/merge-plan-facts.txt` 同源）。

### 步骤 1 — 把 F-01 + F-16 的修法搬到 C（cherry-pick + 手工解 6 个冲突块）

```sh
cd <C 的工作树>            # 建议新开分支：git checkout -b gauntlet/merge-audit-fix
git cherry-pick -n eac070e          # 需要先把 B 的 bundle 取进这个仓库
# 解 4 个文件的 6 个冲突块；audit_queries.rs 的冲突就是「$n 下标 + COUNT 绑定」两处
git add crates/storage/src/{audit_queries,models,job_queries}.rs crates/server/src/{handlers,main}.rs
cargo build --workspace --all-targets --offline && cargo test --workspace --offline
sh qa/harness/server-up.sh false && sh qa/harness/api-checks.sh; sh qa/harness/server-down.sh
```

期望：`DOC-GET-AUDIT-LOGS` 从 `expected=200 actual=500` 变成 **PASS 200**；`api-checks.sh` 的 FAIL 从 2 条降到 1 条（只剩 `DOC-GET-USERS-READONLY`）。

### 步骤 2 — 按文件 graft B 的未提交改动（**一次一个文件，每个都构建+测试**）

推荐顺序（冲突面从小到大）：

1. `crates/storage/src/user_queries.rs`（C 侧只有 1 个提交碰过）
2. `crates/server/src/auth_middleware.rs`（同上）
3. `crates/common/src/alert.rs`
4. `crates/agent/src/metrics.rs` → `crates/agent/src/job_executor.rs`
5. `crates/server/src/ws_handler.rs` → `crates/server/src/grpc.rs`
6. `crates/server/src/handlers.rs` → `crates/server/src/main.rs`（最热，放最后）
7. `Cargo.toml` / `Cargo.lock` / `crates/server/Cargo.toml`

每个文件之后：

```sh
cargo build --workspace --all-targets --offline && cargo test --workspace --offline
git add <file> && git commit -m "[merge] graft B <file>"
```

**不要**整体 `git merge 19d8fbc`——实测 35 个文件冲突且包含 `modify/delete`（见 M4）。

### 步骤 3 — 接回 A 的独有资产（按 M5 的裁决）

- 若选 (c)/(b)：把 `local-wip/crates/common/src/metrics.rs`、`storage/src/conversions.rs`、`tests/integration_test.rs`、`deploy/nginx.conf` 逐个小步搬入，**每个都跑构建+测试**；`integration_test.rs` 需要按 C 的 API 改签名。
- 若选 (a)：跳过，把这三项记入"已知不捡回"清单。

### 步骤 4 — Web（仅当 M5 = b/c）

```sh
# 需要外网：npm install（本机离线，必须由人执行）
cp -r local-wip/web <C>/web
cd web && npm ci && npm run build          # 需要外网/内网 npm 镜像
# 部署面：把 nginx.conf 的 /api 与 /ws 反代指向 C 的 8080
```

验证：`GET /api/nodes` 与 `/ws` 在浏览器里能连；审计页在 F-01 修好后能出数据。

### 步骤 5 — 合流后的全量验证

见 M9。

### 步骤 6 — no-root 合流前必修（NF-01 / NF-02，两处都很小；Leader 裁决 1）

它们**不来自任何一棵树的现有提交**——C 上没有修法，B 的 12 个未提交文件里也没有——所以必须**新写**，改动量都很小：

| 项 | 改哪 | 判据 / 验证命令 |
|---|---|---|
| **NF-02**（1 行） | `deploy/install-agent.sh:80` 的 `pkill -f clusterscope-agent 2>/dev/null \|\| true`（nohup 分支）→ **不要 `pkill -f`**，改成按 PID / 精确匹配（脚本自己写下的 pidfile，或锚定到 `$HOME/.local/bin/clusterscope-agent` 的完整路径） | `grep -n pkill deploy/install-agent.sh`；`bash -n deploy/install-agent.sh`。**验证时不要在一台有常驻 agent 的机器上实跑整条 nohup 分支**（会误杀，见 NF-02）；用 `sh -x` / 只读回显确认匹配范围 |
| **NF-01**（两条小改动） | ① `crates/agent/src/config_loader.rs:9-11`：`-c` 路径不存在时**明确报错**（对齐 server 的 `Config file not found: <path>`，`crates/server/src/main.rs:193`），不再静默回退默认值；② `crates/agent/src/config_loader.rs:40` 附近：创建 `node_id_file` 的**父目录**（现在只建了 log dir） | `env HOME=/tmp/nf01-verify ./target/release/clusterscope-agent -c /tmp/definitely-absent.yaml` → 错误信息里必须出现**缺失的配置路径**；`mkdir -p $HOME/.config` 与否都不再影响退出码（现在是 1 / 124 的差别） |

两条的原始复现命令都在 `qa/qa-report.json` 的 `NF-01`/`NF-02` 条目 `repro` 字段里，修完照抄即可复验。
**注意口径**：`NF-01` 的判定以第 5 阶段为准——作者 harness 的 `NR6 PASS` 是假阳性（判据取了 `grep` 的退出码，
而问候行在崩溃前就打印了），修完 harness 也要一起改成断言进程退出码。

### 步骤 7 — 合流后待办（no-root 维度，Leader 裁决 2：本次审查不改产品代码）

| 待办 | 为什么必须做 | 判据（合流后照抄即可） |
|---|---|---|
| **补用户级 unit / 明确区分两套部署件** | `NRM6`：README:288-289 的 `systemctl --user … clusterscope-server` 在**干净机器**上没有可解析的 unit（仓库 0 个用户级 server unit）；而随仓库发的 `deploy/*.service` 是系统级，非 root 实测装不上 | 选 (a)：`grep -n 'systemctl --user' README.md` 与 `find . -name 'clusterscope-server.service' -not -path './target/*'` **必须同时非空**；选 (b)：README 里每处 `systemctl --user … clusterscope-server` 都改写/删除并注明系统级前提。两种情况都要在 PR 描述里写明选哪条 |
| **文档化「无 root 的数据库路径」** | `NRM7`：README:57/87（`:337` 的 deploy 清单同源）承诺 `docker compose up`，本集群无 docker/docker-compose、无外网 → 不可执行；真正可行的「源码编译 PG 16.4 到 HOME」在 README 里 **0 处**记载 | `grep -n 'docker compose up' README.md` 的每处要么标注「只在有 docker 的节点成立」，要么换成源码编译步骤；重跑 `sh qa/harness/doc-claims-checks.sh`，`NR-09`/`NR-10` 只允许变好 |

这两项都是**文档/部署件**改动（不是产品逻辑），但都属于「无 root 可部署」这条需求的一部分，不能记成「后续再说」。

---

## M7 合流后的质量口径

**事实**：C 现在是硬阈值下的 FAIL——complexity **21/316**（maxCC 23 @ `tui/ui.rs:599 node_panel`）、CRAP **45/316**（maxCRAP 552）、行覆盖率 **20.7%**（1381/6686，storage crate 0%）。kit 口径：`next --profile quality` → CONTINUE，**97 项待修 / 距离 409.505**。

用户已裁决（第 0 阶段）：**关棘轮、硬阈值判定**。本阶段不改这条裁决。

合流对三个指标的影响方向：

| 指标 | 方向 | 理由 |
|---|---|---|
| coverage | **下降** | B 的 12 个文件是 server/agent 的重逻辑（`handlers.rs` +889 行、`main.rs` +624 行），C 侧这些文件覆盖率本来就低（`handlers.rs` 多个函数 0%）；搬进来只会摊薄 |
| complexity | **上升** | B 的 `handlers.rs`/`main.rs` 新增分支（限速、审计、守卫）必然增加圈复杂度 |
| crap | **上升** | 复杂度上升 × 覆盖率不升 = CRAP 上升 |

**三个选项（只能由人裁决）**：
(a) 合流前冻结阈值，合流后按"棘轮模式"（`gauntlet.config.json` 的 `ratchet.enabled: true`）只对改动行严格要求；
(b) 合流后派一次 Cleaner/Hardener 阶段把指标拉回硬阈值（代价：97 项全清，其中 CRAP 一项占距离的 77%）；
(c) 接受指标继续变差，把它记入下一次审查的基线。

**本阶段不改任何阈值、不开棘轮、不写 `quality-accepted.json`。**

---

## M8 哪些发现必须在合流时一起修

判据：**影响数据正确性** > **影响安全边界** > **影响可运维性**。

| finding | 严重度 | 判据 | 定级 | B 线是否已有修法 |
|---|---|---|---|---|
| **F-01** `GET /api/audit-logs` 恒 500 | major | 数据正确性（端点完全不可用） | **合流前必修** | ✅ `eac070e`（实测 cherry-pick 冲突仅 6 块） |
| **F-16** 审计 COUNT 语句零绑定（被 500 掩盖的第二个缺陷） | major | 数据正确性 | **合流前必修** | ✅ 同一个 `eac070e` |
| **NF-01** agent 在「干净 HOME + 缺失 `-c` 文件」下 exit 1，且报错不提配置文件；配置缺失时还会静默回退 | major | **可部署性（无 root 需求）**；错误信息误导运维 | **合流前必修**（Leader 裁决 1，M6 步骤 6） | ❌ 三棵树都没有修法（需新写：明确报错 + 建父目录） |
| **NF-02** `deploy/install-agent.sh:80` 的 `pkill -f clusterscope-agent` 会杀掉同用户**所有** agent（含它没启动的） | major | **共享机器上的破坏性副作用**（本机常驻 agent PID 266643 即活体受害者） | **合流前必修**（Leader 裁决 1，1 行改动，M6 步骤 6） | ❌ 三棵树都没有修法 |
| **F-02** 天级（90 天）历史永不返回、错误被静默吞 | major | 数据正确性 | **合流前必修** | ⚠️ B 的 `f4a8a31 feat: history endpoint serves hourly/daily aggregates beyond 24h` 是**同一问题的另一条修法**，需人比对取舍 |
| **F-07** read-only 鉴权边界与两份文档都不一致 | major | 安全边界 | **合流前必修（至少改文档）** | ⚠️ B 的 `19d8fbc` 提到 "read-only-mode probe" |
| **F-08** 登录无 IP/全局限速 | major | 安全边界 | **合流前必修** | ✅ B-wip `handlers.rs:65,95,200` 有按 IP 的滑动窗口限速器（C 侧 0 命中） |
| **F-09** access token 不可吊销 | major | 安全边界 | **合流前必修** | ✅ B-wip `revoke_all_refresh_tokens`（C 侧 0 命中） |
| **F-10** 审计只有 2 个写入点 | major | 安全边界/可运维 | **合流前必修** | ✅ B-wip `handlers.rs` 13 处 `insert_audit_log` |
| **F-11** 任务参数只有两条校验 | major | 安全边界 | **合流前必修** | ✅ B-wip `MAX_ARGS`/`MAX_ARG_LEN` |
| **F-12** 8 条文档不符 + 默认口令 `admin` | minor | 可运维性（其中默认口令是安全边界） | **合流后单独修**（默认口令建议合流前就改文档/配置） | 部分（`b7f304d security: … weak-password guards`） |
| **F-03** `jobs.pid` 从不落库 | minor | 可运维性 | **合流后单独修** | 部分（B 的 `0c26dcd` reaps orphan process groups） |
| **F-05** 「force → SIGKILL」承诺不存在 | minor | 可运维性 | **合流后单独修** | ✅ B-wip `job_executor.rs` SIGTERM→SIGKILL 升级 |
| **F-06** 15 个死配置键 | major | 可运维性 | **合流后单独修** | ✅ B-wip 激活了 `prometheus_*`、`tls_enabled`、`max_concurrent_ws_clients` 等（`tls_enabled` 12 处命中） |
| **F-13** 没有任何 CI | major | 可运维性 | **合流后单独修**（代价低、收益高） | ❌ 三棵树都没有 |
| **F-14** 没有自动架构检查 | minor | 可运维性 | **只记录** | ❌ |
| **F-15** `retry_count`/`max_retries` 是死列 | minor | 可运维性 | **只记录** | ✅ B 的 `0e79e5a perf: job retries` |
| **F-04** 三个质量闸门 FAIL | major | 质量口径（M7） | **只记录**（本审查不修） | — |

**合流前必修判据总结（第 2 版，2026-10-07 增补）**：`F-01` + `F-16`（同一提交）+ **`NF-01`** + **`NF-02`**
（no-root 维度，改动极小但直接打在「无 root 可部署」这条需求上，见 M6 步骤 6）+ `F-02` + `F-07`~`F-11` + `F-12` 里的默认口令。

**与 no-root 需求的对应**：`NF-01`（agent 缺配置就死、且报错不提原因）与 `NF-02`（安装脚本误杀同用户 agent）
之外，M10 的 `NRM6`（缺用户级 unit）与 `NRM7`（无 root 的 DB 路径未文档化）不是 finding 而是**不变量缺口**，
按 Leader 裁决 2 记入 **M6 步骤 7 的合流后待办**，不在本次审查里改产品代码。

---

## M9 合流后怎么证明「没丢东西」

行为等价清单与验证命令（**全部离线可跑**）：

| 面 | 基线（合流前先记录） | 合流后验证命令 | 判据 |
|---|---|---|---|
| 构建 | `cargo build --workspace --all-targets --offline` exit 0 | 同左 | exit 0 |
| 单元测试 | 44 passed / 0 failed / 0 ignored | `cargo test --workspace --offline` | ≥44 passed，0 failed |
| REST 端点表 | `crates/server/src/main.rs:235-291` 的 22 条路由 | `sh qa/harness/api-checks.sh` | PASS 数**不减少**；F-01 修好后 `DOC-GET-AUDIT-LOGS` 变 PASS |
| TUI 快捷键 | `crates/tui/src/main.rs:85-140` 的 13 个键 | `sh qa/harness/doc-claims-checks.sh`（DOC-TUI-KEY ×13） | 13 条全 PASS |
| gRPC 服务方法 | `proto/clusterscope.proto:433 AgentService` / `:462 CentralService` | `cargo build` + `sh qa/harness/job-e2e.sh` | 任务生命周期 + WS 广播全 PASS |
| DB schema | `crates/storage/src/lib.rs:39-216` 的 **11 张表** | `sh qa/harness/ops-checks.sh`（MIGRATION-*） | 重启幂等、admin 行数 1 |
| 配置键 | `crates/common/src/config.rs` | `sh qa/harness/doc-claims-checks.sh`（DOC-*-YAML-KEY ×17） | 17 条全 PASS（死键数应**下降**） |
| 保留策略 | 原始 24h / 小时 7d / 天级 90d / 日志 30d | `sh qa/harness/long-checks.sh`（约 63 分钟） | CON-10 四条 PASS |
| 文档一致性 | `doc-claims-checks.sh` 当前 **7 条 FAIL** | 同左 | FAIL 数**不增加**；F-12 修完后应下降 |

一条命令的合流后回归：

```sh
cd <合流后的仓库> && \
node .gauntlet/gauntlet.mjs gate --profile quality && \
sh qa/harness/doc-claims-checks.sh && \
sh qa/harness/server-up.sh false && sh qa/harness/api-checks.sh; sh qa/harness/server-down.sh && \
sh qa/harness/extra-checks.sh
```

**注意**：`gate --profile quality` 在硬阈值下**必然 FAIL**（M7），它的作用是提供**逐项数字**给下一次审查对比，不是"必须变绿"。

---

## M10 不变量：合流不得引入新的 root 依赖（NRM1–NRM8 逐条回答）

> 需求原文：「**这个项目是要做一个不用 root 的程序**」。M10 把它拆成 8 条**合流后必须仍然成立**的不变量
> （`qa/merge-plan-requirements.md` 的 M10 展开），本节逐条回答：**判据 → 现在的事实（命令与输出）→ 合流时要做什么**。
> 命令一律在**合流后的仓库根目录**执行（下文的 `R`）；每条都与第 5 阶段增补落进 `qa/qa-report.json` 的检查同源
> （`Q300`–`Q322`、`Q330`），原始输出在 `qa/evidence/no-root-verify*.txt`。
> 本节的**总判定**是：**4 条现在成立（NRM1/NRM3/NRM5 + NRM8 的前置）、1 条部分不成立（NRM2，= NF-01）、
> 2 条不成立（NRM6/NRM7）、1 条现在无法评估（NRM4，B 的 12 个文件不在本树上）。**

### 一、逐条判定

| # | 不变量（合流后必须成立） | 现在（合流前）的判定 | 合流时要做的动作 |
|---|---|---|---|
| **NRM1** | 运行时零特权：普通用户起 server/agent/TUI，8080/50051 监听、`/api/health`=200、TUI 在 pty 渲染 | ✅ **成立**（`Q300`/`Q301`/`Q302`/`Q318`/`Q320`） | 每次 graft 后重跑 `sh qa/harness/no-root-checks.sh`（NR1/NR2/NR3/NR18/NR20 必须仍 PASS） |
| **NRM2** | 零配置文件也不崩：server 靠 `POSTGRES_URL`/`JWT_SECRET`/`AUTH_REQUIRED` 可起（`NR-21`）；agent 缺 `-c` 文件时不读系统路径、日志落 HOME（`NR-06`/`NR-06b`） | ⚠️ **部分不成立**：`NR-21` ✅（`Q321`）、`NR-06b` ✅（`Q306`），但 `NR-06` ❌ → **NF-01**（`Q305`：干净 HOME + 缺 `-c` 文件 = exit 1） | **合流前必修 NF-01**（M6 步骤 6）；修完复跑 `Q305` 的复现命令 + `lsof -p <pid> \| grep -cE '/etc/clusterscope\|/var/(lib\|log)/clusterscope'` 必须为 0 |
| **NRM3** | 产品代码与用户级脚本里**0 处硬编码系统路径** | ✅ **成立**：`crates/` 与 `deploy/install-agent.sh`、`deploy/tui.sh` 里共 **3 处**，全部是**可覆盖的默认值**（见下） | 合流后同一条 grep 的命中数**不得增加**；新增的每一处都要有可覆盖的 CLI/env 理由 |
| **NRM4** | B 的 12 个未提交文件逐一验证不引入 root 依赖 | ⏳ **现在无法评估**：那 12 个文件**不在本树上**（未提交、不在任何 bundle 里，见 M1） | 步骤 2 graft 每个文件**之前/之后**各跑一次见下的循环命令；命中即 FAIL，必须逐条解释或改回 |
| **NRM5** | 合流不得新增特权原语（`setuid`/`setgid`/`pre_exec` 提权/`chown` 系统路径/`sudo`/`pkexec`；新增依赖不得引入要求系统级权限的路径） | ✅ **成立**：**0 个真特权原语**（正确模式下命中 2 行，见下，是进程组设置不是提权） | 合流后重跑正确模式的 grep；`git diff <base> --stat -- Cargo.toml crates/*/Cargo.toml` 后逐个新依赖查用途 |
| **NRM6** | 两套部署件的矛盾必须**修掉或明确区分**（不能继续「README 说 user、仓库发 system」） | ❌ **不成立**：`NR-13`（README 5 行 `systemctl --user`、仓库 0 个用户级 server unit）+ `NR-15`（系统级 unit 非 root 装不上：`systemctl link` → `Interactive authentication required`，`cp` → `Permission denied`） | **M6 步骤 7 待办**（Leader 裁决 2）：选 (a) 补一个用户级 unit，或 (b) 把 `deploy/*.service` 改名/注明系统级前提，并改 README |
| **NRM7** | 文档承诺与实测口径一致：README:87 的 `docker compose up` 要么改成与本集群一致的说明，要么标注「需要 docker 的节点才成立」 | ❌ **不成立**：README **:57 / :87**（`:337` 同源）承诺 compose；本机无 docker/docker-compose、无外网；可行路径「源码编译 PG 到 HOME」**README 0 处**记载 | **M6 步骤 7 待办**；重跑 `sh qa/harness/doc-claims-checks.sh`，`NR-09`/`NR-10` 只允许变好 |
| **NRM8** | 合流后仍可复现证明：`sh qa/harness/no-root-checks.sh` 全 PASS、`node .gauntlet/gauntlet.mjs test` 的 44/44 不减少、M9 的行为等价清单同时通过 | ⏳ **前置已就绪**：基线 `tests: 44/44`；no-root 脚本作者口径 `PASS=23 FAIL=0`（但其中 NR6 是**假阳性**，见下） | 合流后三条命令的输出**贴在 PR 描述里**；任何新的 FAIL 都要有对应 finding 条目（不得静默） |

### 二、逐条的事实与命令

**NRM1（成立）**——`id -u` = 3000；`/proc/<server-pid>/status` 的 `Uid: 3000 3000 3000 3000`、
`CapEff: 0000000000000000`（零特权，绑 8080/50051 也不需要 `CAP_NET_BIND_SERVICE`，两个端口都 >1024）：

```sh
grep -E '^(Uid|Gid):' /proc/<pid>/status            # → 3000 3000 3000 3000
grep '^CapEff:' /proc/<pid>/status                  # → 0000000000000000
ss -ltnH | grep -E ':(8080|50051)[[:space:]]'       # → 两个 LISTEN（owner = 该 pid）
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/health   # → 200
```

证据：`qa/evidence/no-root-verify2-e-nr1-socket-ownership.txt`、`no-root-verify-12-nr01-unprivileged-bind.txt`、
`no-root-verify-13-nr18-tui-pty.txt`、`no-root-verify-14-nr20-nvml-sysfs.txt`。

**NRM2（NF-01 部分不成立）**——`NR-21` 的 env-only server 实测成立（`argv` 里没有配置参数、`health=200`、
`lsof` 在 `/etc/clusterscope`、`/var/lib`、`/var/log` 下命中 **0**），`NR-06b` 的日志目录也确实建在 HOME。
但 `NR-06` 在**它自己的判据场景**里不成立：

```sh
H=/tmp/nf01; rm -rf $H; mkdir -p $H
env HOME=$H timeout 8 ./target/release/clusterscope-agent -c /etc/clusterscope/agent.yaml; echo exit=$?   # → exit=1
mkdir -p $H/.config
env HOME=$H timeout 8 ./target/release/clusterscope-agent -c /etc/clusterscope/agent.yaml; echo exit=$?   # → exit=124（活着）
env HOME=$H ./target/release/clusterscope-agent -c /tmp/definitely-absent.yaml 2>&1 | tail -3            # 报错只提 node_id
```

机制：`crates/agent/src/config_loader.rs:9-11` 只看 `exists()` → 静默用 `AgentConfig::default()`；
`:40` 只建 log dir、**不建** `node_id_file` 的父目录 → `node_identity.rs:27` 写失败 → exit 1；
报错里"缺失的配置文件"出现次数 = **0**。**另一个二进制不一致**：server 对缺失配置是
`Error: Config file not found: <path>` 直接退出（`crates/server/src/main.rs:193`），agent 却沉默地换默认值。

**NRM3（成立，3 处可覆盖默认值）**：

```sh
grep -rn '/etc/clusterscope\|/var/lib/clusterscope\|/var/log/clusterscope\|/usr/local/bin' \
  crates deploy/install-agent.sh deploy/tui.sh | grep -v '^#'
crates/server/src/main.rs:186:        .unwrap_or("/etc/clusterscope/server.yaml");      # 可被 argv[1] 覆盖
crates/agent/src/main.rs:15:    #[arg(short, long, default_value = "/etc/clusterscope/agent.yaml")]  # 可被 -c 覆盖
crates/common/src/config.rs:38:                .unwrap_or_else(|| PathBuf::from("/etc/clusterscope"))   # 可被配置键覆盖
```

= **3 处**（与合流前基线同数）；`deploy/install-agent.sh`、`deploy/tui.sh` 的**非注释行里 0 处**。
合流后的唯一要求是**这个数不增加**。

**NRM4（现在无法评估）**——B 的 12 个文件只存在于 `/public/tianyuyang/code/ClusterScope` 的工作树里
（M1）；合流 graft 之前先备份（M6 步骤 0 已完成），然后对每个文件跑：

```sh
for f in Cargo.lock Cargo.toml crates/agent/src/job_executor.rs crates/agent/src/metrics.rs \
         crates/common/src/alert.rs crates/server/Cargo.toml crates/server/src/auth_middleware.rs \
         crates/server/src/grpc.rs crates/server/src/handlers.rs crates/server/src/main.rs \
         crates/server/src/ws_handler.rs crates/storage/src/user_queries.rs; do
  git diff -- "$f" | grep -nE '/etc/|/var/lib|/var/log|/usr/local|pre_exec|setuid|setgid|chown|CAP_' && echo "HIT $f";
done
```

命中即 FAIL。**注意**：这 12 个文件里 `Cargo.lock`/`Cargo.toml` 的变化还要按 NRM5 的后半句
（新增依赖是否要求系统级权限）逐个人工确认。

**NRM5（成立；并纠正一处模式假象）**——`qa/merge-plan-requirements.md` 里给的那条
`grep -rnE 'setuid\|setgid\|pre_exec\|pkexec\|sudo \|chown' crates/` 在 GNU grep 下**匹配空集**
（ERE 里 `\|` 是字面竖线，不是"或"），它的「0 命中」**不是测量结果**（这条已作为 discovery `N14` 原样并入）。
用正确的 ERE 复跑：

```sh
grep -rnE 'setuid|setgid|pre_exec|pkexec|sudo |chown|setsid' crates/ | wc -l   # → 2（同一处调用）
crates/agent/src/job_executor.rs:128:        cmd.pre_exec(|| {
crates/agent/src/job_executor.rs:129:            libc::setsid();
```

这是**给子进程建新会话/进程组**（配合取消时按进程组 kill），**不是提权**；`getcap` 显示二进制上没有文件能力。
所以 NRM5 的结论**成立**，但依据换成上面这条命令。

**NRM6（不成立，合流后待办）**——两套部署件的实测差异：

| 面 | 用户级（`install-agent.sh`） | 系统级（`deploy/server.service`、`deploy/agent.service`） |
|---|---|---|
| 路径 | `~/.local/bin`、`~/.config/clusterscope`、`~/.config/systemd/user` | `/usr/local/bin`、`/etc/clusterscope`、`/var/lib/clusterscope`、`/var/log/clusterscope-*` |
| 归属 | 当前用户 | `User=clusterscope`（**本机不存在该用户**） |
| 目标 | `systemd --user`（`WantedBy=default.target`）或 `nohup` | `WantedBy=multi-user.target` |
| 额外依赖 | 无 | `After=network.target postgresql.service redis.service`（`redis` 代码里从未被读；本机 PG 编译在 HOME，`postgresql.service` 不存在 → **N11**） |
| 非 root 实测 | ✅ 可用（本机 2026-09-02 起 user unit active） | ❌ `systemctl link` → `Failed to link unit: Interactive authentication required.`；`cp` → `Permission denied` |

而 README 有 **5 行** `systemctl --user`（288、289、290、291、321），其中 288/289 管的是 **server**，
仓库里 **0 个**用户级 server unit（`find . -name 'clusterscope-server.service' -not -path './target/*'` = 0）。
本机那份 `~/.config/systemd/user/clusterscope-server.service`（`ExecStart=$HOME/.local/bin/clusterscope-server $HOME/.config/clusterscope/server.yaml`、
`WantedBy=default.target`、2026-08-10 建立、当前 disabled/inactive）是**手写私货**，不属于仓库 →
「用户级跑 server」这条路**可行但没被交付**。

**NRM7（不成立，合流后待办）**——README 的 compose 承诺有两处（不止一处）：

```sh
grep -n 'docker compose up\|docker-compose' README.md
57:| PostgreSQL | v16+(server 必需;可用 `deploy/docker-compose.yml` 一键起) |
87:无 root 时可用 `docker compose up`(`deploy/docker-compose.yml`,只含 postgres + server)。
337:deploy/          # systemd、docker-compose、install-agent.sh、tui.sh
command -v docker || echo docker-absent     # → docker-absent（podman 在，但零镜像、无外网）
```

可行路径是「源码编译 PostgreSQL 16.4 到 HOME」（本机就是这么跑的：`pg_ctl status` exit 0、
`select version()` = `PostgreSQL 16.4`），而 README **0 处**提到它。

**NRM8（前置就绪，判据在合流后）**——基线：`tests: 44/44`；`sh qa/harness/no-root-checks.sh` 的作者口径
`PASS=23 FAIL=0`（`NR-17` 是 `na` 但脚本记 PASS，这是脚本的口径）。**但作者脚本的 `NR6` 那一行是假阳性**：
`qa/harness/no-root-checks.sh:171-177` 用 `grep -q 'ClusterScope Agent starting'` 判 NR6、**从不读进程退出码**，
而问候行在崩溃前就打印了 → 所以合流后**不能**只跑作者脚本就算证明，要同时用第 5 阶段的独立复推
（`qa/harness/nr-verify*.sh`，证据 `qa/evidence/no-root-verify*.txt`）。

```sh
sh qa/harness/no-root-checks.sh              # 期望 PASS=23 FAIL=0（NR-13/NR-15 在处置后应变 PASS）
sh qa/harness/nr-verify.sh                   # 独立复推（判据比作者脚本严：读退出码）
node .gauntlet/gauntlet.mjs test             # 期望 ≥44 passed / 0 failed
sh qa/harness/doc-claims-checks.sh           # FAIL 数不增加
```

### 三、部署前提（合流后必须一起写进 README 的 5 条）

这几条是「无 root 可用」真正成立的前提，现在 README 都没有写全：

| 前提 | 事实 | 合流后怎么处理 |
|---|---|---|
| **HOME 可写** | `NR-19`：`HOME` 只读时 agent `Error: Failed to create log directory` 直接 exit 1（硬失败）；server 不受影响（只写 stderr） | 写进 README 的部署前提 |
| **端口都 >1024** | 8080/50051 不需要 `CAP_NET_BIND_SERVICE`；但默认配置的 `prometheus_addr` 是 0.0.0.0:9090（该键还是死键） | 写清端口表 |
| **`jwt_secret` 守卫** | `NR-07`：不给配置也不用环境变量时 server **拒绝启动**（`refusing to start: jwt_secret is missing/too weak`），README 没写 | 写进 README 的「最小启动」 |
| **linger（每机配置）** | 见下（双向记录） | 写成部署前提，不要替集群假设 |
| **数据库从哪来** | 见 NRM7 | 见 M6 步骤 7 |

**linger 双向记录（原样写清，不替集群做假设）**：

- **`linger=yes` → 用户级服务活过登出**：本机实测 ✅。`loginctl show-user tianyuyang` → `Linger=yes`、`State=active`；
  探针 unit 的 `MainPID` 在会话结束后仍存活（`NR-11`/`Q311`）；更强的证据是本机自 **2026-09-02** 起常驻的
  用户级 agent（PID 266643）跨过了每一次登出。
- **环境事实（必须原样写明）**：本机当前 `Linger=yes` **很可能是 Leader 前期探测时执行 `loginctl enable-linger` 造成的**，
  **不是集群默认值**；探测初期读到的是 `no`。`loginctl enable-linger` 需要 root/管理员（本机无 sudo，
  且本次**不许**改集群配置），所以 `linger=no` 的分支在本机**无法复现**。
- **`linger=no` → 随登出而死**：语义上，最后一个会话结束时 logind 会停掉该用户的 per-user manager，
  它的所有 unit（含 `Restart=always`）一起被杀 → **干净节点上用户级 agent 会随登出而死**；
  而开启 linger 通常需要管理员/root。这是「无 root 承诺」的一条**边界**，必须写清。
- **`install-agent.sh` 只探测 `systemctl --user show-environment`、从不检查 `Linger`**（`deploy/install-agent.sh:60`）
  → 在没有 linger 的节点上会**静默**装出一个登出即死的服务。合流时应把这条写进安装脚本的提示或 README。

### 四、M10 的验收口径（一句话）

在一台**没有 root、没有 docker、没有外网**的节点上，用合流后的代码把 server + agent + TUI 跑起来
（`sh qa/harness/no-root-checks.sh` 全 PASS，只允许已在 PR 里写明处置的 `NR-13`/`NR-15` 两条），
并且 `crates/**` 里没有任何代码路径要求写 `/etc`、`/var/lib`、`/var/log`、`/usr/local/bin`；
`NRM1`–`NRM8` 的逐条命令与输出**贴在合流 PR 描述里**。

---

## 风险与回滚

| 风险 | 触发条件 | 缓解 | 回滚 |
|---|---|---|---|
| B 的 12 个未提交文件丢失 | 有人 `git checkout -- .` / `git clean` / 重装工作树 | **步骤 0 已完成**：`backup-b-wip/b-wip.tar.gz`（12 个成员，sha256 `8ef25e55…`）＋ 逐文件 sha256 清单 | 从 `backup-b-wip/b-wip.tar.gz` 解开（同输入重建哈希一致） |
| cherry-pick `eac070e` 时把 B 的其他改动带进来 | 误用 `git merge 19d8fbc`（实测 35 个冲突文件） | 只用 `cherry-pick -n` + 按文件 `git add` | `git cherry-pick --abort` |
| 合流后 REST 契约漂移（web 依赖的 10 个端点） | 接回 web 但 server 路由改了 | 合流后先跑 `api-checks.sh` 再开浏览器 | 分支回退 |
| 质量指标继续变差掩盖新缺陷 | 合流引入新代码 | 每次 graft 后单独提交，`git bisect` 可用 | 单提交 revert |
| 共享机器上误杀别人的进程 | 用 `pkill`/`killall` | **只按 PID 文件停**（`qa/harness/*-down.sh`）；本机有一个不属于本次审查的常驻 agent（PID 266643），本阶段未做任何干预。**注意：仓库自带的 `deploy/install-agent.sh:80` 恰好违反这条**（`pkill -f clusterscope-agent` 会杀掉同用户所有 agent，见 **NF-02**）→ 合流前必修（M6 步骤 6） | — |
| 合流把「不用 root」这条需求做没了 | 新代码/新依赖要求系统路径或特权 | **M10 的 `NRM1`–`NRM8`**：每次 graft 后跑 `sh qa/harness/no-root-checks.sh` + `crates/` 的系统路径 grep（命中数不得增加） | 单提交 revert；`NRM4` 的循环命令逐文件定位 |

| `qa/harness/ops-checks.sh:112` 全表清空 `node_metrics` | 在共享 PG 上跑 ops-checks | 已知夹具卫生问题（N9）；跑之前先备份或改脚本只删自己的 node_id（**改脚本需人确认**） | 从备份恢复 |

---

## 附录：本文件用到的探测命令与产物

```sh
# 1) 两个 bundle 的 refs
git bundle list-heads /public/tianyuyang/code/ClusterScope-review/node-line.bundle
git bundle list-heads /public/tianyuyang/code/ClusterScope-review/gh-line.bundle

# 2) 提交图（在探测仓库 /tmp/probe-verify 里，已把两个 bundle 取进 refs/remotes/b/master 与 refs/remotes/c/master）
#    完整脚本：qa/evidence/line-fork-verify.sh；原始输出：qa/evidence/line-fork-verify.txt
git merge-base b/master c/master                       # → f8ac7267f834a2b65da1aaf9c065f2de3c524250（唯一共同祖先）
git rev-parse --short b/master; git rev-parse --short c/master   # → 19d8fbc / f9c080b
git rev-list --count b/master; git rev-list --count c/master     # → 22 / 40
git log --oneline c/master..b/master                   # → 15 个 B 独有
git log --oneline b/master..c/master                   # → 33 个 C 独有
git merge-base --is-ancestor b/master c/master; echo $?      # → 1  ← 不是祖先；初版把空输出误读成 YES（见勘误 E-1）
git merge-base --is-ancestor c/master b/master; echo $?      # → 1  ← 反向也不是
git rev-list --count f8ac726                           # → 7（共同前缀：f46b6a9 … f8ac726）
git rev-list --count d1586b6..b/master                 # → 12（本地裸仓库 `ClusterScope.git` 停在 d1586b6）
git diff --stat f266f2f 75c3f98                        # → 空：B/C 前 3 个提交 tree 相同（同 parent，仅提交者/时间戳不同）

# 3) 文件级差异
diff -rq local-wip/crates gh-line/crates               # → 39 同名不同 + 4 A 独有
for f in agent.service agent.yaml.example install-agent.sh server.service server.yaml.example tui.sh; do cmp -s local-wip/deploy/$f gh-line/deploy/$f && echo SAME $f || echo DIFF $f; done

# 4) B 的 12 个未提交文件的打包与校验
tar czf backup-b-wip/b-wip.tar.gz -C /public/tianyuyang/code/ClusterScope -T backup-b-wip/list.txt
sha256sum backup-b-wip/b-wip.tar.gz                    # → 8ef25e55af7cb612162cfc9887fd88dd52dba55260aa1c7508f984756fd9b4df

# 5) cherry-pick 可行性探测（只读，不留改动）
git cherry-pick -n eac070e                             # → 4 个文件 6 个冲突块
git diff --name-only --diff-filter=U
```

原始输出留在 `gauntlet-out/merge/`（gitignored）：`a-vs-c.txt`、`a-vs-c-2.txt`、`b-wip/`（B 的 12 个文件解包）。
