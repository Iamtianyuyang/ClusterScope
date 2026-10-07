# QA：M6 三线合流（步骤 1 / 2 / 3 / 5）—— 执行程序与判据

> **范围**：严格按 `report/merge-plan.md` 的 **M6**。基线是 **`master` @ `8601ac9`**（审查产物 PR #2 + 无 root 修复 PR #3 都已进主线）。
> 目标：把 **B 线**（`b/master` = `19d8fbc`）的修复与 **A 线**（`../local-wip/`）的独有资产合流进主线。
> **不做**：步骤 4（Web / `deploy/nginx.conf` —— M5 裁决「先 TUI-only」）、M7 质量口径（97 项欠账 / 距离 409.5 是已记录的审查结论，本轮不还债、不重算成 PASS）。
>
> 本文既是**第 2 阶段（Coding）每一步的验收判据**，也是**第 5 阶段（QA）的复跑清单**。
> 每张表的「证实约束」列指向 `qa/constraints.json` 里本轮追加的 `MRG6-*`（既有 104 条 + `FIX-01`…`FIX-14` 一字不动）。

---

## 0 前置：怎么构建、真实产物在哪、怎么跑

| 项 | 值 |
|---|---|
| 工作树 | `/public/tianyuyang/code/ClusterScope-review/merge-m6`，分支 `gauntlet/merge-m6`，合流基线 `8601ac9` |
| 构建（闸门口径） | `cargo build --workspace --all-targets --offline`（**无外网**，一律 `--offline`；`export PATH=$HOME/.cargo/bin:$PATH`） |
| 测试（闸门口径） | `cargo test --workspace --offline` 或 `node .gauntlet/gauntlet.mjs test`（后者带覆盖率与验收场景对照） |
| 真实产物 | `target/release/{clusterscope-server,clusterscope-agent,clusterscope-tui}`（`cargo build --release --workspace --offline`，实测 24.5s） |
| 数据库 | PostgreSQL 16.4，`postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`（`psql`/`pg_ctl` 在 `../pg16/bin/`；`qa/harness/env.sh` 已设好） |
| 本轮判据程序 | `sh qa/harness/merge-m6-checks.sh [--static]`（M9 + M10 的可执行版，证据落 `gauntlet-out/qa/evidence/merge-m6-*`） |
| 无 root 回归 | `sh qa/harness/no-root-fixes-checks.sh`（12 段 F1–F12，含 F11「只追加」与 F12「改动范围」） |
| 冻结副本（只读，不要去动） | `../backup-b-wip/b-wip.tar.gz`（12 个成员，sha256 `8ef25e55…b4df`）+ `list.txt` + `sha256-manifest.txt` + `PROVENANCE.txt` |

**三条硬规矩**（`qa/README.md`，本轮同样适用）：

1. **只按 PID 文件停进程**（`server.pid` / `agent.pid` / `qa/harness/*-down.sh`）：这台机器是共享的，
   `pkill -f clusterscope` 会误杀别人的进程；本机有一个**不属于本次工作**的常驻 agent（**PID 266643**，systemd --user），
   任何一步都不得碰它、它的 unit 或它的配置。
2. **不要用 `| head` 截断 kit 命令**：SIGPIPE 会让 node 提前死掉，而报告是最后才写的。
3. **不要并发跑两条 kit 命令**：它们会互相覆盖 `gauntlet-out/junit.xml` / `lcov.info`。

---

## 1 合流前基线（2026-10-07 在本工作树实测；M9 的对照面）

| # | 面 | 实测基线 | 命令 | 合流后的判据 |
|---|---|---|---|---|
| 1 | 构建 | exit 0 | `cargo build --workspace --all-targets --offline` | exit 0（**不得**引入需要联网的新依赖） |
| 2 | 测试 | **59 passed / 0 failed**（`F10` 证据 `no-root-fixes-F10-tests.txt`） | `cargo test --workspace --offline` | ≥ 59 passed、0 failed |
| 3 | 验收场景 | 6 场景（`features/no_root_agent_config.feature`），`gate --profile coder` 显示 59/59 + 6/6 | `node .gauntlet/gauntlet.mjs test` | 验收场景**只增不减**：6 + 本轮新增 22 = 28，全部要有**通过的**同名测试 |
| 4 | REST 矩阵 | **18 PASS / 2 FAIL**：`DOC-GET-USERS-READONLY`（期望 200 实际 **401**）、`DOC-GET-AUDIT-LOGS`（期望 200 实际 **500**） | `sh qa/harness/server-up.sh false && sh qa/harness/api-checks.sh; sh qa/harness/server-down.sh` | PASS 数**不减少**（≥18）；`DOC-GET-AUDIT-LOGS` **必须翻成 PASS**（= F-01 修好的机器判据） |
| 5 | 文档一致性 | **71 PASS / 9 FAIL**（见 §5 翻转登记表） | `sh qa/harness/doc-claims-checks.sh` | FAIL 数**不增加**（≤9）；`DOC-CODE-SIGKILL-EXISTS`、`DOC-CODE-FORCE-OPTION` 必须翻成 PASS |
| 6 | TUI 快捷键 | 13 条 `DOC-TUI-KEY`（合流前已 PASS） | `doc-claims-checks.sh` | 13/13 PASS |
| 7 | 配置键 | 17 条 `DOC-{SERVER,AGENT}-YAML-KEY`（合流前已 PASS） | `doc-claims-checks.sh` | 17/17 PASS；死键数**应下降** |
| 8 | 迁移 / schema | `public` 下 **11 张表**、`users` 里 `admin` **1 行** | `psql -c "\dt"`；`select count(*) from users where username='admin'` | 重启幂等：仍 11 张表、admin 仍 1 行 |
| 9 | 无 root 修复（四项） | `no-root-fixes-checks.sh` **PASS=12 FAIL=0**（M6 轮口径，见 §8） | `sh qa/harness/no-root-fixes-checks.sh --no-slow` | **12/12**；F1–F11 与合流前同结论，F12 按 M6 轮口径判 |
| 10 | NRM3（硬编码系统路径） | **实质命中 3 处**（全部是可覆盖默认值）；同一 grep 全部命中 **7 行**（另 4 行是注释与测试夹具） | `grep -rn '/etc/clusterscope\|/var/lib/clusterscope\|/var/log/clusterscope\|/usr/local/bin' crates deploy/install-agent.sh deploy/tui.sh` | 实质命中 ≤3、全部命中 ≤7；新增的每一处都要有「可被 CLI/env 覆盖」的理由 |
| 11 | NRM5（特权原语） | `setuid\|setgid\|pre_exec\|pkexec\|sudo \|chown\|setsid` 共 **2 行**（`job_executor.rs:128` 的 `pre_exec` + `:129` 的 `libc::setsid()`，是**给子进程建进程组**，不是提权）；提权族本身 **0 行** | 见 §5 的 NRM5 命令 | 提权族仍 0 行；进程组族 ≤2 行 |
| 12 | 约束清单 | **118 条**（104 审计 + `FIX-01`…`FIX-14`），`F11` 的「零删行」成立 | `git diff --numstat 7ca587a -- qa/constraints.json` → `140 0` | 追加 `MRG6-*` 后仍是 **纯新增**（`--numstat` 的第二列必须是 0） |

> **口径提醒**：`merge-plan.md` / `qa/README.md` 里写的「44 tests」「api-checks 2 条 FAIL」「doc-claims 7 条 FAIL」
> 都是**更早的两轮**（gh-line 线）的数字。本轮的对照面**只能是上面这张表**（同一棵树、同一天实测）。

---

## 2 步骤 1 —— 把 F-01 + F-16 的修法搬到主线（`eac070e`）

**为什么**：`GET /api/audit-logs` 恒 500（F-01）与审计 COUNT 语句零绑定（F-16）都在 B 的 `eac070e` 里一次修掉；
M4 实测：整体 merge 会炸 35 个文件，但只 cherry-pick 它改动的 5 个文件时，冲突收敛到 **4 个文件 6 个冲突块**。

```sh
cd /public/tianyuyang/code/ClusterScope-review/merge-m6
git cherry-pick -n eac070e                                   # 只会动 5 个文件（audit_queries/job_queries/models/handlers/main）
git diff --name-only --diff-filter=U                          # 期望恰好 4 个文件
```

| # | 操作 / 判据 | 期望 | 证实约束 |
|---|---|---|---|
| 1.1 | 解冲突时的**硬规矩**：占位符一律取 C 的 `$n` 语义（`WHERE 1=1` 不占参数位）、绑定顺序与 `$n` 对齐；**SELECT 与 COUNT 两条语句都必须绑参数**（F-16） | 4 个文件、6 个块全部解完；`models.rs` 自动合并 | MRG6-01 / MRG6-02 |
| 1.2 | `cargo build --workspace --all-targets --offline` | exit 0 | MRG6-01 |
| 1.3 | `cargo test --workspace --offline` | ≥59 passed / 0 failed | MRG6-02 |
| 1.4 | 4 条新验收场景（`features/merge_m6_audit_queries.feature`）必须有同名测试并全绿 | `node .gauntlet/gauntlet.mjs test` 的 ACCEPTANCE 一栏 0 missing | MRG6-03 |
| 1.5 | `sh qa/harness/server-up.sh false && sh qa/harness/api-checks.sh; sh qa/harness/server-down.sh` | `DOC-GET-AUDIT-LOGS: PASS expected=200 actual=200`；PASS 数 **19**（18 + 这 1 条）；仍剩 `DOC-GET-USERS-READONLY` 1 条 FAIL | MRG6-01 |
| 1.6 | 提交：`git add crates/storage/src/{audit_queries,models,job_queries}.rs crates/server/src/{handlers,main}.rs && git commit -m "[merge] step1: F-01+F-16 audit endpoint (cherry-pick eac070e)"` | 每个绿点一次提交 | — |
| — | **回滚** | `git cherry-pick --abort`（未 add 前）；已提交则 `git revert <sha>` | — |

---

## 3 步骤 2 —— 按文件 graft B 的 12 个未提交文件

**素材**：`../backup-b-wip/b-wip.tar.gz`（**冻结副本，同输入重建哈希一致**）。解包到 gitignored 的
`gauntlet-out/m6/b-wip/`（第 1 阶段已解好一次，命令见下），**不要去读 B 的活工作树**。

```sh
mkdir -p gauntlet-out/m6/b-wip && tar xzf ../backup-b-wip/b-wip.tar.gz -C gauntlet-out/m6/b-wip
# 参考差异（B-wip vs 本树）：
for f in $(cat ../backup-b-wip/list.txt); do printf '%-45s' "$f"; diff -u "$f" "gauntlet-out/m6/b-wip/$f" | grep -c '^[+-]'; done
```

**推荐顺序**（冲突面从小到大，取自 M6）：`user_queries.rs` → `auth_middleware.rs` → `alert.rs` →
`agent/metrics.rs` → `agent/job_executor.rs` → `ws_handler.rs` → `grpc.rs` → `handlers.rs` → `main.rs` →
`Cargo.toml` / `Cargo.lock` / `crates/server/Cargo.toml`。

**一次一个文件**，每个文件都走下面这张表（**这就是「怎么证明没丢东西」的逐文件口径**）：

| # | 操作 | 判据 | 证实约束 |
|---|---|---|---|
| 2.1 | 逐块 graft（`diff -u <本树文件> gauntlet-out/m6/b-wip/<同路径>` 作参考），**保留 C 侧独有修复**（去重 key、status 映射、orphan 回收、marker 重放、`remove_rule` 级联、TUI/NVML 版 metrics、`$n` 占位符） | 出现「B 的改法 vs C 的改法」二选一时，**取语义更全的一侧并写下理由**；两侧都保留得下就都保留 | MRG6-04 |
| 2.2 | 记录残留差异：`diff -u crates/.../X.rs gauntlet-out/m6/b-wip/crates/.../X.rs > gauntlet-out/qa/evidence/merge-m6-graft-$(basename X.rs).diff` | 残留差异只允许是「C 侧 API/结构不同」造成的适配，不允许出现 B 侧功能被静默丢弃 | MRG6-04 |
| 2.3 | **NRM4**（合并前必修的不变量）：`git diff 8601ac9 -- <grafted file> \| grep -nE '/etc/\|/var/lib\|/var/log\|/usr/local\|pre_exec\|setuid\|setgid\|chown\|CAP_'` | **无命中**；有命中必须逐条解释或改回（两个 `Cargo.*` 还要按 NRM5 后半句人工确认新依赖不要求系统级权限） | MRG6-05 |
| 2.4 | `cargo build --workspace --all-targets --offline` | exit 0 | MRG6-01 |
| 2.5 | `cargo test --workspace --offline` | ≥59 passed / 0 failed（新增的验收测试逐条累加） | MRG6-02 |
| 2.6 | 提交 `[merge] graft B <file>` | 每文件一次提交 → 任何一个文件出问题都能单提交 revert（M6 的风险表） | — |

**每个文件修好的 finding 与要走的验收场景**（M8 的「合流前必修」判据）：

| graft 文件 | 修好的 finding | 对应的新验收场景（`features/merge_m6_auth_hardening.feature`） |
|---|---|---|
| `crates/storage/src/user_queries.rs` | **F-09**（令牌批量吊销）、最后管理员保护、令牌按摘要存储、单次消费 | `revoking_all_sessions_…`、`a_refresh_token_can_only_be_consumed_once`、`refresh_tokens_are_stored_as_digests_…`、`the_last_enabled_administrator_…` |
| `crates/server/src/auth_middleware.rs` | F-09 的请求侧（令牌失效后的行为） | 同上（走 REST 时由 `api-checks` 的 `SEC-*` 段覆盖） |
| `crates/common/src/alert.rs` | 告警规则删除级联（B 版 `remove_rule_instances` vs C 版 `remove_rule`） | `job-e2e.sh` 的 `ALERT-RULE-DELETE-CASCADE` |
| `crates/agent/src/metrics.rs` | 与 C 侧 NVML/per-core 版的取舍（**取语义更全的一侧**，两侧字段都要能上报） | `doc-claims` 的键清单 + `ops-checks` 的指标族 |
| `crates/agent/src/job_executor.rs` | **F-05**（SIGTERM→SIGKILL 升级）、重试/孤儿回收 | `cancelling_a_job_whose_process_ignores_sigterm_escalates_to_sigkill`、`cancelling_a_job_that_exits_on_sigterm_…` |
| `crates/server/src/ws_handler.rs` | 每 IP WS 连接数上限 | `job-e2e.sh` 的 `WS-*` 四条 |
| `crates/server/src/grpc.rs` | **F-11**（`MAX_ARGS`/`MAX_ARG_LEN`）、上报侧审计 | `a_job_submission_with_too_many_arguments_is_rejected`、`…argument_over_the_length_limit…`、`…at_the_argument_limits_is_accepted` |
| `crates/server/src/handlers.rs` | **F-08**（登录 IP/全局限速）、**F-10**（审计写入点 2→13） | 4 条限速场景 + `merge-m6-checks.sh` 的 `M6-12`（审计覆盖）与 `M6-13`（429 正判据） |
| `crates/server/src/main.rs` | **F-06**（`tls_enabled` / `prometheus_*` / `max_concurrent_ws_clients` 等死键激活）、限速器后台清理 | `the_configuration_keys_that_used_to_be_dead_are_read_from_the_file`、`enabling_tls_without_certificate_or_key_paths_is_refused_with_a_clear_error` |
| `Cargo.toml` / `Cargo.lock` / `crates/server/Cargo.toml` | 依赖清单（B 侧新增） | **离线**：`cargo build --offline` exit 0 就是可用性证明；`git diff 8601ac9 -- Cargo.toml crates/*/Cargo.toml` 逐个新依赖写明用途 | MRG6-06 |

---

## 4 步骤 3 —— 接回 A 的独有资产（按 M5 裁决 = 先 TUI-only）

只接这三样（`deploy/nginx.conf` 与 `web/` 属于步骤 4，**本轮不做**）：

| # | 资产 | 接法与判据 | 证实约束 |
|---|---|---|---|
| 3.1 | `crates/common/src/metrics.rs`（190 行：指标类型 + `MetricsAggregation`） | `cp ../local-wip/crates/common/src/metrics.rs crates/common/src/` + 在 `crates/common/src/lib.rs` 加 `pub mod metrics;`；查重名（C 侧无同名模块 / 无同名类型） | MRG6-07 |
| 3.2 | `crates/storage/src/conversions.rs`（26 行：`node_metrics_to_proto`） | `cp` 后在 `crates/storage/src/lib.rs` 加 `pub mod conversions;`；**按 C 的 API 改签名**（C 的 `models::NodeMetricsRow` 字段与 `protocol::NodeMetricsReport` 以合流后的定义为准） | MRG6-07 |
| 3.3 | `tests/integration_test.rs`（A 的集成测试，10 个用例） | 按 C 的 API 改签名后放入 `tests/`（本树此前没有 `tests/` 目录）；**只移植 A 的用例，不删任何既有测试** | MRG6-07 / MRG6-08 |
| 3.4 | 每条资产后：`cargo build --workspace --all-targets --offline` + `cargo test --workspace --offline` | exit 0 / ≥59+新增 passed、0 failed | MRG6-01 / MRG6-02 |
| 3.5 | 对应验收场景（`features/merge_m6_legacy_assets.feature`）全绿 | 4 条场景都有同名通过的测试 | MRG6-07 |

> **口径（M3 的裁决，别读成「删测试」）**：A 的 `integration_test.rs` 里有两条用例依赖 `common::dedup` / `common::sequence`，
> 而这两个模块在 **M3** 里判定为「**被 C 的 `LruCache` / `AtomicU64` 方案替代，不捡回**」——它们对应的产品代码**根本不进主线**，
> 所以那两条用例**不移植**。这不是「删/跳过既有测试」：`tests/` 在本树此前不存在，且 F10/F12 的「被删 `#[test]` 行」判据
> （`git diff 8601ac9 -- crates tests | grep '^-.*#\[test\]'`）保持为空。

---

## 5 步骤 5 —— 合流后的全量验证（M9 表 + M10 不变量）

**一条命令的回归**（`merge-m6-checks.sh` 把下面几乎全部自动跑完）：

```sh
cd /public/tianyuyang/code/ClusterScope-review/merge-m6
node .gauntlet/gauntlet.mjs build && node .gauntlet/gauntlet.mjs test       # 不要并发跑这两条
sh qa/harness/merge-m6-checks.sh                                            # M6-01 … M6-15（默认跳过 F1/F5 的 systemd 段）
sh qa/harness/no-root-fixes-checks.sh                                       # 12/12（可选：M6_FULL_NOROOT 见 §8）
sh qa/harness/no-root-checks.sh --no-slow                                   # 作者口径的 NR1–NR21 复跑（独立复推：nr-verify*.sh）
sh qa/harness/doc-claims-checks.sh                                          # FAIL ≤ 9
```

### 5.1 M9 行为等价清单

| 面 | 判据（合流前基线见 §1） | 命令 |
|---|---|---|
| 构建 | exit 0 | `cargo build --workspace --all-targets --offline` |
| 测试 | ≥59 passed / 0 failed | `cargo test --workspace --offline` |
| 验收场景 | 28 个场景都有通过的测试（6 旧 + 22 新） | `node .gauntlet/gauntlet.mjs test` |
| REST 端点表 | PASS ≥18，且 `DOC-GET-AUDIT-LOGS` 由 FAIL 转 PASS | `merge-m6-checks.sh` 的 `M6-11` |
| TUI 快捷键 | 13/13 PASS（`DOC-TUI-KEY`） | `M6-09`；TUI 真渲染见 `auth-tui-checks.sh` 的 `TUI-RENDERS-HEADER` |
| gRPC 任务生命周期 + WS 广播 | `job-e2e.sh` 0 FAIL（含 `WS-CONNECTED/SUBSCRIBED/METRICS-PUSH/JOB-UPDATE-PUSH`） | `M6-14` |
| DB schema | 重启后 11 张表、admin 1 行 | `M6-15` |
| 配置键 | 17/17 PASS；死键数下降 | `M6-10` + `ops-checks.sh` 的 `DEADKEY` 行 |
| 保留策略（慢） | `CON-10` 四条 PASS（**约 63 分钟**，标为可选/慢速） | `sh qa/harness/long-checks.sh`（`nohup` 跑，掉线的 ssh 不会杀掉测量） |
| 文档一致性 | FAIL ≤9 | `M6-08` |

### 5.2 M10 不变量（NRM1–NRM8）

| # | 合流后的判据 | 命令 / 落点 |
|---|---|---|
| NRM1 | 运行时零特权（uid 3000、端口 >1024、`/api/health`=200） | `no-root-checks.sh` 的 `NR1/NR2/NR3/NR18/NR20` 仍 PASS |
| NRM2 | 零配置也不崩：`NR-21`（env-only server）、`NR-06`/`NR-06b`（agent 缺 `-c`） | `no-root-fixes-checks.sh` 的 `F2/F3/F4`（这三条已经把这套语义固化） |
| NRM3 | 系统路径默认值不增加 | `M6-05`（实质 3 处、全部命中 7 行） |
| NRM4 | 12 个 graft 文件逐个不引入 root 依赖 | §3 的 2.3（逐文件执行，命中即 FAIL） |
| NRM5 | 不新增特权原语、新依赖不要求系统级权限 | `M6-06`；`git diff 8601ac9 --stat -- Cargo.toml crates/*/Cargo.toml` 逐个新依赖写用途 |
| NRM6 / NRM7 | 用户级 unit 与无 root DB 路径的文档化（**上一轮已落地**） | `no-root-fixes-checks.sh` 的 `F5`–`F9`（本轮只要求不回归） |
| NRM8 | 可复现：`no-root-checks.sh` + `no-root-fixes-checks.sh` + M9 清单同时成立 | `M6-07` / `M6-12`…`M6-15` |

### 5.3 翻转登记表（**每个 PASS/FAIL 翻转都必须在这里有理由**）

| 检查 | 合流前 | 合流后 | 为什么 |
|---|---|---|---|
| `api-checks` `DOC-GET-AUDIT-LOGS` | FAIL（500） | **PASS（200）** | F-01 + F-16 被步骤 1 修好（M8 的「合流前必修」） |
| `doc-claims` `DOC-CODE-SIGKILL-EXISTS` | FAIL | **PASS** | 步骤 2 的 `job_executor.rs` 带来了真的 SIGKILL 升级（F-05） |
| `doc-claims` `DOC-CODE-FORCE-OPTION` | FAIL | **PASS** | 同上（`"force"` 参数） |
| `extra-checks` `SEC-13-NO-IP-RATE-LIMIT` | PASS（它断言"429 出现次数 = 0"） | **FAIL（预期）** | **它不是回归**：这条检查记录的是 finding（F-08「没有限速」）。修好后同一命令必然变红。**替代判据**是 `M6-13`（12 次同源失败登录必须出现 429）——两者必须同时存在，不许只保留一个 |
| `extra-checks` `SEC-13-ALL-ATTEMPTS-401` | PASS（20/20 都是 401） | **FAIL（预期）** | 同上：出现 429 后不可能再全是 401 |
| `concurrency-checks` `CON-JOB-PID-PERSISTED` | FAIL（finding F-03） | 可能翻成 PASS | 步骤 2 的 `job_executor.rs` 会把 pid 落库（B 侧的 F-03 部分修法）；若仍 FAIL，按 M8「合流后单独修」处理，**不算回归** |
| `doc-claims` `DOC-INSTALL-AGENT-USES-C` | FAIL（上一轮 no-root 修复改了 `install-agent.sh` 的写法） | 仍 FAIL | **本轮不改**：它检查的是 `install-agent.sh` 里的 `-c $HOME/.config/...` 字面量，属于上一轮遗留的文档一致性欠账，判据只要求「FAIL 数不增加」 |
| `doc-claims` 其余 6 条 FAIL（LICENSE / architecture 保留策略 / api.md 三条缺文档 / 另 1 条） | FAIL | 仍 FAIL | 见 `qa/docs-consistency.qa.md` 与 `F-12`；本轮不合（§9） |

---

## 6 本轮的检查程序：`qa/harness/merge-m6-checks.sh`

```sh
sh qa/harness/merge-m6-checks.sh            # 全量（自己起停 server/agent，只按 PID 文件）
sh qa/harness/merge-m6-checks.sh --static   # 只跑不需要 server/agent 的段（M6-01…M6-10）
```

| ID | 检查 | 判据 | 证实约束 |
|---|---|---|---|
| `M6-01` | 构建 | `cargo build --workspace --all-targets --offline` exit 0 | MRG6-01 |
| `M6-02` | 测试 | rc 0、failed 0、passed ≥59 | MRG6-02 |
| `M6-03` | 测试未被弱化 | 相对 `8601ac9` 没有被删的 `#[test]` 行 | MRG6-08 |
| `M6-04` | 范围守卫 | 改动全部落在 M6 允许集内、冻结面零改动（**独立实现**，与 `F12` 互为对照） | MRG6-09 |
| `M6-05` | NRM3 | 实质命中 ≤3、全部命中 ≤7 | MRG6-05 |
| `M6-06` | NRM5 | 提权族 0 行、进程组族 ≤2 行且 `pre_exec` 旁有 `setsid` | MRG6-06 |
| `M6-07` | 无 root 四项修复 | `no-root-fixes-checks.sh` 12/12 | MRG6-10 |
| `M6-08` | 文档一致性 | FAIL ≤9 且 SIGKILL/force 两条翻 PASS | MRG6-11 |
| `M6-09` | TUI 键 | 13/13 | MRG6-11 |
| `M6-10` | 配置键 | 17/17 | MRG6-11 |
| `M6-11` | REST 矩阵 | PASS ≥18 且审计端点 PASS | MRG6-03 |
| `M6-12` | 审计覆盖（F-10） | 一次登录 + 建任务 + 停任务 + 建用户（+ 失败登录）后，`audit_logs` 里新动作种类 ≥3，且端点可见 | MRG6-12 |
| `M6-13` | 登录限速（F-08 正判据） | 12 次同源失败登录出现 429；与 `extra-checks` 的 SEC-13 翻转成对 | MRG6-13 |
| `M6-14` | 任务生命周期 + WS | `job-e2e.sh` 0 FAIL、PASS ≥10 | MRG6-14 |
| `M6-15` | 迁移幂等 | 重启后 health 200、11 张表、admin 1 行 | MRG6-15 |

> `M6-12` 的判据里**不要**用 `ops-checks.sh` 那样清空整表（该脚本 `:112` 的 `delete from node_metrics` 是已知夹具卫生问题 N9，跑之前要先备份或改脚本 —— 改脚本需人确认）。`M6-15` 是自写的只读判据。

---

## 7 harness 的树定位修正（为什么必须改，怎么证明没改判定）

**问题**：`qa/harness/env.sh` 把仓库根**硬编码**成 `.../ClusterScope-review/gh-line`；`no-root-checks.sh`、`nr-verify*.sh` 也各自硬编码了同一个路径。
在**别的**工作树（本轮的 `merge-m6`）里跑这些脚本，它们会**静默地测旧树**（拿 gh-line 的二进制、配置与源码当证据）——
对合流验证来说是致命的假证据。

**修法**（只动「找根」这一行，**一行判定都没改**）：

```sh
# env.sh
if [ -z "${REPO:-}" ]; then
  if [ -n "${HERE:-}" ]; then REPO="$(cd "$HERE/../.." && pwd)"
  else echo "env.sh: 无法确定仓库根…" >&2; return 2; fi
fi
# no-root-checks.sh / nr-verify.sh / nr-verify2.sh / nr-verify3.sh
R="${R:-$(cd "$(dirname "$0")/../.." && pwd)}"
```

**怎么证明没改判定**：`git diff 8601ac9 -- qa/harness/env.sh qa/harness/no-root-checks.sh qa/harness/nr-verify*.sh`
只允许出现上面这几行与注释；每行的 `CHECK`/`A "…"` 断言集合逐行不变。复跑：在 `merge-m6` 里
`sh qa/harness/doc-claims-checks.sh` 的输出必须与本文 §1 第 5 行一致（71 PASS / 9 FAIL），
在 `gh-line` 里跑同样的命令必须仍是它自己的数字 —— 两棵树的证据不再互相冒充。

---

## 8 F12 的轮次口径（为什么 F12 要按轮次选允许集）

**事实**：`F12`（改动范围）判定的是「相对某个基线，改动是否落在该轮的允许集内」。

* **no-root 修复轮**：基线 `7ca587a`，允许集 = `FIX-13` 原文 + `FIX-14`（`gauntlet-tools/*`）+ `FIX-15`（`demo/*`）。
* **M6 合流轮**：基线 `8601ac9`，允许集 = `crates/**`、`tests/*`、`Cargo.toml/lock`、`deploy/*`（**除 `deploy/nginx.conf`**）、
  `README.md`、`docs/*`、`features/*`、`qa/*`、`demo/*`、`report/*`、`GAUNTLET.md`、`gauntlet-tools/*`。
  负例自检换成 M6 轮**仍然必须判越界**的 5 条：`.gauntlet/gauntlet.mjs`、`gauntlet.config.json`、`gauntlet-baseline.json`、
  `web/src/main.tsx`、`deploy/nginx.conf`；正例自检 2 条：`crates/server/src/handlers.rs`（合流目标）与 `demo/*`（FIX-15）。

**为什么必须分轮**：合流**注定**要改 `crates/server/**` 与 `crates/storage/**`（步骤 1/2 的全部内容），
而 no-root 轮的允许集把它们列为「越界」。让 `F12` 在本树上继续用旧口径，等于要求「合流不许改合流目标」。

**合流前实测（未改脚本时）**：`sh qa/harness/no-root-fixes-checks.sh` 在本工作树给出 **PASS=11 FAIL=1**，
`F12` 唯一失败项是 `report/no-root-fixes-comment.md`、`report/no-root-fixes.html`（上一轮报告阶段的产物，
旧允许集里没有 `report/*`）。**任务书里写的「12/12」与基线实测不符，这里如实记录。**

**两种口径怎么复跑**（都不会被静默改写）：

```sh
sh qa/harness/no-root-fixes-checks.sh                        # 本树 = M6 轮口径（默认，取本树能解析到的最新一轮基线）
NR_FIX_BASE=7ca587a sh qa/harness/no-root-fixes-checks.sh    # 旧口径；在本树上 F12 的两条范围断言**预期 FAIL**
                                                             # （原因就是合流本身改了 server/storage，不是修复回归）
```

**状态：本修正沿用 `FIX-14`/`FIX-15` 的先例（spec 阶段落地 + 负例自检不放松 + 追加约束记录），
需 Leader 追认**；若 Leader 不认，回退方式是在 `no-root-fixes-checks.sh` 里把 `BASE` 默认值改回 `7ca587a`
（其余改动与本条无关，可单独保留）。

---

## 9 决策记录：F-02 / F-07 / F-12 的取舍

| finding | 裁决 | 理由 | 后续路径 |
|---|---|---|---|
| **F-02** 天级（90 天）历史永不返回、错误被静默吞 | **本次不合入**（不搬 B 的 `f4a8a31`） | ① 它不在 M6 的三步里（步骤 1 只搬 `eac070e`，步骤 2 只搬 12 个未提交文件）——本轮的任务书明确说「不要扩大」；② C 侧**已经有**一套 `history_tiers` 实现与 4 条通过的单元测试（`handlers.rs` 的 `test_history_tiers_*`），B 的 `f4a8a31` 是**同一问题的另一条修法**，搬进来等于替换一套已被测试固定的实现——M8 自己写的是「需人比对取舍」；③ 判据面（`ops-checks.sh` 的 `HISTORY-DAILY-SOURCE`）是**已记录的 finding**，不受「PASS 数不减少」约束 | 单独一轮：先固化 C 现状（`ops-checks.sh` 的 4 条 `HISTORY-*` + 3 档探针），再决定「在 `history_tiers` 上定点修」还是「换成 B 的聚合路径」，两条路都要带自己的等价性证据 |
| **F-07** read-only 鉴权边界与两份文档都不一致 | **本次不合入**（既不改鉴权代码，也不改文档口径） | ① 改代码 = 改安全边界（要新测试 + 两份文档同步 + `api-checks` 的期望矩阵整体重写）；② 改文档 = 改口径，但 `api-checks` 的 `DOC-GET-USERS-READONLY` 期望值写死在脚本里（实测 401），**改文档不会让它变绿**，反而把「哪份文档算数」的问题留在原地；③ B 的对应物只是 `19d8fbc` 的一句「read-only-mode probe」，不是完整修法 | 单独一轮，二选一并写进 PR：**(a)** 代码放开（`GET /api/users` 等在 read-only 下 200）或 **(b)** 文档收紧 + 把 `api-checks` 的期望随裁决一起改（改判据必须 Leader 批准）。两种都要先把 read-only 期望矩阵固化成表 |
| **F-12** 默认口令（代码默认 `admin`，README 写 `admin123`） | **不改产品默认值**；只做「文档补强」 | 改默认值会牵动首启动守卫、`deploy/*.yaml.example`、`server-up.sh`、NR-07 的 jwt 守卫与 12/12 的 no-root 检查，属于**产品行为变更**，本轮既无预算也无对应验收场景；而 `doc-claims` 有两条 grep 恰好盯着这两行（`DOC-ADMIN-PASSWORD-DEFAULT` 要 README 里有 `admin123`、`DOC-ADMIN-PASSWORD-CODE-DEFAULT` 要代码里是 `"admin"`），**动任何一行都会让 FAIL 数增加** | 文档补强（MRG6-16）：在 README 的安全提示里写清「代码默认口令是 `admin`，首次启动后必须改」，**不得**改动上面那两条 grep 盯着的行；产品侧改成「不给口令就拒绝启动」留到单独一轮（与 NR-07 的守卫一起设计） |

---

## 10 证据落点

| 证据 | 位置 |
|---|---|
| M6 判据程序输出 | `gauntlet-out/qa/evidence/merge-m6-checks.txt` + 同目录 `merge-m6-*.txt` |
| 逐文件 graft 的残留差异 | `gauntlet-out/qa/evidence/merge-m6-graft-<file>.diff` |
| 无 root 回归 | `gauntlet-out/qa/evidence/no-root-fixes-*.txt` |
| 合流前基线（本研究树实测） | `gauntlet-out/m6/`（gitignored：`api-checks-baseline.txt`、`doc-claims-baseline.txt`、`no-root-fixes-prebaseline.log`、`release-build.log`） |
| 冻结副本与哈希 | `../backup-b-wip/`（`b-wip.tar.gz` sha256 `8ef25e55…b4df`、`list.txt`、`sha256-manifest.txt`、`PROVENANCE.txt`） |
| QA 报告（第 5 阶段写） | `qa/qa-report.json`（追加 `constraint: "MRG6-xx"` 的条目，既有 132 条检查不动） |
