# 三棵树合流方案（第 6 阶段交付 · M1–M9 逐题）

> 审查对象：**C = GitHub 已发布线** `f9c080b`（TUI-only，40 个提交，`version 0.1.1`）。
> 本文件回答 `qa/merge-plan-requirements.md` 的 M1–M9，每题给**事实**（带命令与证据）与**建议**（可执行、代价可估）。
> 事实来自第 5 阶段已核验的 `qa/evidence/merge-plan-facts.txt`，以及本阶段在父目录 `ClusterScope-review/` 的
> `node-line.bundle` / `gh-line.bundle` / `local-wip/` 上重新跑的探测（`gauntlet-out/merge/*.txt`）。
> **`M5` 是产品决策，本文件只给选项与代价，不替人拍板。**

三棵树的代号沿用 `qa/merge-plan-requirements.md`：

| 代号 | 位置 | HEAD | 状态 |
|---|---|---|---|
| **A** | 本地 Windows `D:\code\ClusterScope`；素材快照 `ClusterScope-review/local-wip/` | `f8ac726` + 未提交 Web 工作 | 只此一份，未提交、未推送 |
| **B** | node `/public/tianyuyang/code/ClusterScope`（裸仓库 `/public/tianyuyang/git/ClusterScope.git`） | `19d8fbc` | 22 个提交；12 个文件未提交；从未推送到 GitHub |
| **C** | `ClusterScope-review/gh-line` = GitHub `master` | `f9c080b` | 已发布、可构建、44 个测试 |

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
| 关键结构事实 | **B 的 HEAD `19d8fbc` 是 C 的祖先**（`git merge-base --is-ancestor b/master c/master` → YES）。即 C = B 的全部提交历史 **+ 33 个新提交**；B 相对 GitHub 没有"分叉"，是**落后**。 |
| B 的 12 个未提交改动 | 不在任何提交里、也不在 bundle 里；只存在于 `/public/tianyuyang/code/ClusterScope` 的工作树（`git status --short` 12 个 `M` 行，与清单逐字一致） |

B 独有的 15 个提交里，**前 3 个（`f266f2f` / `15b47a7` / `d1586b6`）在 C 里有同内容的等价提交**（C 侧哈希 `75c3f98` / `963ed9c` / `d43af1f`，提交信息逐字相同）——说明 C 是把这些改动**重放**（rebase/cherry-pick）上去的，不是分叉。剩下 12 个提交（`09f7460` … `19d8fbc`）是 B 的**独有修复**，其中相当一部分在 C 里被独立地重新实现过（例如 C 的 `f4a8a31`、`57938b4`、`0b87b0d`、`5f210c2`），但**修法与覆盖范围不同**（见 M8）。

**结论**：三条线不是"三棵平行的树"，而是**一条主干（`f8ac726` → B → C）加一棵离线的工作树（A）**。
"B 领先裸仓库 12 且从未推送"指的是 **B 的本地裸仓库 `/public/tianyuyang/git/ClusterScope.git`**（它停在 `d1586b6`），
**不是** GitHub —— 相对 GitHub，B 落后 33 个提交。

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

**基线取 C**（已发布、可构建、44 测试通过），分 6 步，每步都有验证命令：

### 步骤 0 — 冻结 B 的 12 个未提交文件（**先做，不可跳**）

```sh
cd /public/tianyuyang/code/ClusterScope-review
mkdir -p backup-b-wip
git -C /public/tianyuyang/code/ClusterScope status --short | awk '{print $2}' > backup-b-wip/list.txt
tar czf backup-b-wip/b-wip.tar.gz -C /public/tianyuyang/code/ClusterScope -T backup-b-wip/list.txt
sha256sum backup-b-wip/b-wip.tar.gz     # 本次实测：8ef25e55af7cb612162cfc9887fd88dd52dba55260aa1c7508f984756fd9b4df
```

校验：`tar tzf b-wip.tar.gz | wc -l` 必须是 **12**；12 个文件的 sha256 见本报告附录（`qa/evidence/merge-plan-facts.txt` 同源）。

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

**合流前必修判据总结**：`F-01` + `F-16`（同一提交）+ `F-02` + `F-07`~`F-11` + `F-12` 里的默认口令。

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

## 风险与回滚

| 风险 | 触发条件 | 缓解 | 回滚 |
|---|---|---|---|
| B 的 12 个未提交文件丢失 | 有人 `git checkout -- .` / `git clean` / 重装工作树 | **步骤 0 先打包 + sha256** | 从 `backup-b-wip/b-wip.tar.gz` 解开 |
| cherry-pick `eac070e` 时把 B 的其他改动带进来 | 误用 `git merge 19d8fbc`（实测 35 个冲突文件） | 只用 `cherry-pick -n` + 按文件 `git add` | `git cherry-pick --abort` |
| 合流后 REST 契约漂移（web 依赖的 10 个端点） | 接回 web 但 server 路由改了 | 合流后先跑 `api-checks.sh` 再开浏览器 | 分支回退 |
| 质量指标继续变差掩盖新缺陷 | 合流引入新代码 | 每次 graft 后单独提交，`git bisect` 可用 | 单提交 revert |
| 共享机器上误杀别人的进程 | 用 `pkill`/`killall` | **只按 PID 文件停**（`qa/harness/*-down.sh`）；本机有一个不属于本次审查的常驻 agent（PID 266643），本阶段未做任何干预 | — |
| `qa/harness/ops-checks.sh:112` 全表清空 `node_metrics` | 在共享 PG 上跑 ops-checks | 已知夹具卫生问题（N9）；跑之前先备份或改脚本只删自己的 node_id（**改脚本需人确认**） | 从备份恢复 |

---

## 附录：本文件用到的探测命令与产物

```sh
# 1) 两个 bundle 的 refs
git bundle list-heads /public/tianyuyang/code/ClusterScope-review/node-line.bundle
git bundle list-heads /public/tianyuyang/code/ClusterScope-review/gh-line.bundle

# 2) 提交图（在探测仓库 /tmp/probe 里，已把两个 bundle 取进 refs/b/* 与 refs/c/*）
git merge-base b/master c/master                       # → f8ac726
git rev-list --count b/master / c/master               # → 22 / 40
git log --oneline c/master..b/master                   # → 15 个 B 独有
git log --oneline b/master..c/master                   # → 33 个 C 独有
git merge-base --is-ancestor b/master c/master && echo YES   # → YES

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
