# 三线合流 M6：把 B 线的修复与 A 线的独有资产合进主线

> 需求原文：**安装 https://github.com/Iamtianyuyang/small-and-beautiful 到该项目 然后全部合并**
> 本 PR 是 **三线合流**（`report/merge-plan.md` 的 **M6 步骤 1 / 2 / 3 / 5**）的**编码 + QA + 报告**产物。
> 分支 `gauntlet/merge-m6` @ `9817ae9` → **目标分支 `master`**（本轮基线 `8601ac9` = 审查产物 PR #2 + 无 root 修复 PR #3 都已在主线）。
> **本 PR 合并后，`master` 即为三线合流的终态**（A 线未提交的 Web 工作与 `deploy/nginx.conf` 不在其中：M5 裁决「先 TUI-only，web 走独立分支」）。
> 证据包（单文件、离线可看）：**`report/m6-merge.html`**；原始输出：`qa/evidence/`、`qa/qa-report.json`。

## 这次合流了什么

| # | 来源 | 内容 | 关键文件 |
|---|---|---|---|
| 1 | B 的 `eac070e`（cherry-pick） | **F-01** `GET /api/audit-logs` 恒 500 → 200；**F-16** 审计 `COUNT` 语句重新绑参（`total` 不再回落成全表数）。顺带在合流中发现 F-01 的第二重错因（B 侧占位符 `+1` 与 `LIMIT/OFFSET` 不一致） | `crates/server/src/handlers.rs`、`crates/storage/src/audit_queries.rs` |
| 2 | B 的 12 个未提交文件（**逐文件 graft，三方合并**，每文件一次构建+测试+提交） | **F-08** 登录限速（按「源地址+账号」的窗口 + 全局预算）；**F-09** 令牌批量吊销 / 单次消费 / 摘要存储 / 随用户级联删除；**F-10** 审计覆盖面 2 → 4 种动作；**F-11** 任务提交上限 `MAX_ARGS=256` / `MAX_ARG_LEN=4096`；**F-05** `SIGTERM → SIGKILL` 升级 + README:439 承诺的 `?force=true`；**F-06** 死配置键激活；每 IP 的 WebSocket 连接上限 | `handlers.rs`、`user_queries.rs`、`auth_middleware.rs`、`grpc.rs`、`ws_handler.rs`、`job_executor.rs`、`alert.rs`、`agent/metrics.rs`、`main.rs` |
| 3 | A 的独有资产（M5 裁决 = 先 TUI-only） | `crates/common/src/metrics.rs`、`crates/storage/src/conversions.rs`、`crates/common/tests/integration_test.rs`（7 例）；**依赖 `dedup`/`sequence` 的 2 例按 M3 裁决不移植**。两样资产标为「已就位、暂无产品消费者」（MRG6-24 / F-27），不假装已接进运行时 | 见左 |
| 4 | 本轮新增规格与判据 | 23 个 Gherkin 场景（审计查询 / 认证加固 / 任务安全 / 旧资产）+ `qa/merge-m6.qa.md` + `qa/harness/merge-m6-checks.sh`（M6-01…M6-15）+ `qa/constraints.json` 追加 `MRG6-01…MRG6-24`（**纯插入，零删行**，既有 118 条逐字节未动） | `features/merge_m6_*.feature`、`qa/**` |

**形式**：30 个提交、**0 个 merge 提交**（不整体 `git merge 19d8fbc` —— 方案 E-1 实测那会炸 35 个文件）、116 文件（+14923 / −902）。每个 graft 都可单独 review、可单独回退。

## 怎么验的

| 判据 | 结果 |
|---|---|
| `node .gauntlet/gauntlet.mjs gate --profile coder` | **PASS** — spec **29 场景** · build ✅ · tests **98/98（0 failed）** · acceptance **29/29, missing 0** |
| `sh qa/harness/merge-m6-checks.sh` | **PASS=15 FAIL=0**（M6-01…M6-15：构建/测试/范围/无 root 不变量/文档/审计覆盖/限速正判据/job-e2e/迁移幂等） |
| `sh qa/harness/no-root-fixes-checks.sh --no-slow` | **PASS=12 FAIL=0**（四项无 root 修复不回归；NRM3 系统路径 3 ≤ 3、NRM5 特权原语 0、进程组族 2 ≤ 2） |
| `demo/13` … `demo/16`（可回放录像） | **4/4 exit 0** — 审计端点筛选矩阵 / force-cancel 与 SIGTERM 升级 / 锁定 429 + cluster-info null / F-28 串行化实验 |
| REST 安全矩阵（`api-checks.sh`） | **19 PASS / 1 FAIL** — 唯一 FAIL 是 `DOC-GET-USERS-READONLY expected=200 actual=401`（= **F-07 的安全边界取舍，需要人拍板**） |
| 文档一致性（`doc-claims.sh`） | **73 PASS / 6 FAIL**（基线 9；`DOC-CODE-SIGKILL-EXISTS`、`DOC-CODE-FORCE-OPTION` 如期由 FAIL 翻 PASS） |
| 任务生命周期 e2e（`job-e2e.sh`） | **15 PASS / 0 FAIL**（含 WebSocket 广播 4 条、告警级联删除） |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | **rc=0**；`cargo fmt --all --check` 干净 |
| 质量闸门（`gate --profile quality`，**既有欠账，本轮不还**） | warnings 0 ✅ / tidy 0 ✅ / duplication 0% ✅ / scope 35 文件 ✅；**complexity ❌ 34 项（maxCC 23）· crap ❌ 56 项（maxCRAP 552）· coverage ❌ 31.2%** |

**三轮 QA**（`qa/qa-report.json` 的 Q501–Q553）：

- **round 1 = FAIL**：抓出一条**相对 master 的安全回归 F-23**——锁定账号应答 429，合流后变成 401（把「账号被锁」和「口令错」变成同一种回答）。更刺眼的是：上一轮提交信息**声称已修**，实际 diff 里**只有注释变了**（`return Err(StatusCode::UNAUTHORIZED)` 一个字没动）。另有 MRG6-17（README 默认口令补强）没做、MRG6-22 的勘误数字不准。
- **返工**（`3230e8b` / `d26ada0` / `5ceee91`，无 amend、无 reset）：F-23 改回 429；F-24（`cluster/info` 无数据必须 JSON null，不许假装 0）；MRG6-17 README 补强；F-25 两项（clippy 告警 + 重复代码克隆）。
- **round 2 = PASS**：逐条独立复跑闭合 F-23/F-24/MRG6-17/F-25；另如实新记 **F-28**（`clippy --all-targets` 的 9 条 MutexGuard 告警不在基线 `8601ac9`，与编码阶段自述的「既有」不符）与 **F-29**（质量欠账残差变大）。
- **round 3 = PASS**：F-28 在 `7800865` 闭合（`std::sync::Mutex` → `tokio::sync::Mutex`，**只动锁管线、断言逐字未动**）；QA 用**反证实验**裁决了「是不是收窄作用域弱化测试」——收窄变体 lint 干净但并行 5/5 失败，工作树版本并行 5/5 全绿且墙钟与串行同级 → 原方法成立。

**测试咬出的 3 个 B 未提交代码真 bug**（都由 QA 在真实产物上独立复现，`qa/evidence/m6-qa5-*.txt`）：

1. `SELECT COUNT(*) … FOR UPDATE` 被 PG 逐字拒绝 → 「最后一个启用管理员」守卫**从未运行**（专属库实测：拒绝路径 400/400/400，允许路径 200）；
2. `process_starttime()` 读 `/proc/<pid>/stat` **下标 20（那是 vsize）** 而非 19（starttime）→ 取消任务**从不发信号**（数值证明：下标 20 在 4 次采样里 233541632 → 434880512 变化，下标 19 全程不变）；
3. 合流后 `/audit-logs` **重复注册** → server 启动即 panic（`180f416` 上实跑：退出码 101，`Overlapping method route. Handler for 'GET /audit-logs' already exists`）。

## 开口项（随 PR 交给人拍板）

| 编号 | 是什么 | 为什么本轮不合 | 后续路径 |
|---|---|---|---|
| **F-02** | 天级 90 天历史永不返回 | B 的 `f4a8a31` 是**另一条修法**，与 C 侧已有实现（+4 条测试）冲突，二选一要人定 | 并排对比两条修法，选定后单独小 PR |
| **F-03** | `jobs.pid` 从不落库 | 需要改 **冻结的 `proto/**`**（`JobStatusUpdate.pid/exit_code`） | 单独一轮：proto 加 optional 字段 → 双侧落库 → 加 e2e 判据 |
| **F-07** | 只读账号看不到用户列表（401 vs 文档期望 200） | **改代码 = 改安全边界**，机器判不出对错 | (A) 文档跟随代码改 401；(B) 放开只读可见 + 字段脱敏 + 新判据 —— 请安全口径负责人拍 |
| **F-12** | 产品默认管理员口令 `admin` 太弱 | 改默认值会让已有部署升级后登不进去；本轮只做 **README 补强**（纯插入，代码默认值未动） | 单独立项：首次启动生成随机口令 / 强制改密流程 |
| **F-27** | A 的两样资产「已就位、暂无产品消费者」 | 接进运行时 = 重写一条有测试的 SQL 聚合或改一个已文档化的响应形状，超出「只合流」范围 | 等真正需要它的那一轮；测试与场景都保留，没删任何东西 |
| **F-29** | M7 质量欠账残差 **592.706 / 117 项**（master 409.505 / 97 项） | 本轮是**合流**不是**还债**；一部分原因是「被测到的代码变多了」（316 → 435 个函数），但 complexity 23 → 34、CRAP 45 → 56 也确实含合流代码的贡献，**不掩饰** | M7 轮：`next --profile quality` 拿清单，按 CRAP 降序拆（TOP：`tui/ui.rs:599 node_panel` 552 / `server/main.rs:544 run_background_tasks` 380 / `tui/ui.rs:1024 draw_process` 342） |

**本轮明确不做**：步骤 4（`web/`、`deploy/nginx.conf`，M5 = web 走独立分支）；`proto/**` 冻结；M7 质量口径（不还 97 项、不重算成 PASS、棘轮 `enabled:false` 未动）。

## 复跑（先读共享机器前提）

```sh
cd /public/tianyuyang/code/ClusterScope-review/merge-m6
export PATH=$HOME/.cargo/bin:$PATH
node .gauntlet/gauntlet.mjs gate --profile coder     # 期望 GATE coder: PASS
sh qa/harness/merge-m6-checks.sh                     # 期望 M6-CHECKS: PASS=15 FAIL=0
sh qa/harness/no-root-fixes-checks.sh --no-slow      # 期望 PASS=12 FAIL=0
```

> ⚠️ **共享机器**：本机有一个**不属于本工作**的常驻 agent（**PID 266643**，systemd --user）——不要碰它、它的 unit 或它的配置；停进程只按自己的 PID 文件（不要 `pkill -f clusterscope`）。
> `no-root-fixes-checks.sh` 的**全量**版本（不带 `--no-slow`）会覆盖那个常驻 agent 的 unit 文件，**本机口径一直是 `--no-slow`**（本 PR 全程如此）。不要并发跑两条 kit 命令（共用 `junit.xml` / `lcov.info`）。PostgreSQL 16.4：`postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`；远端无外网，cargo 一律 `--offline`。

## 审阅入口

- **5 分钟人类审阅路线**：`report/m6-merge.html` 第 ⑦ 节（先看 ① 结论与 ⑤ 闸门面板 → ② 8 项修复的「前/后」列 → ③ 3 个 bug + ④ round 1 的安全回归 → ⑥ 开口项 → 任选一段演示录像）。
- **演示录像**（远端，未入库）：`gauntlet-out/evidence/demos/{13,14,15,16}-*.html`（可回放终端录像 + 命令与退出码）。
- **原始证据**：`qa/evidence/m6-qa5-*.txt`（round 1，含 3 个 bug 的复现）/ `m6-qa6-*.txt`（round 2）/ `m6-qa7-*.txt`（round 3，含反证实验）/ `m6-qa7-gates.txt`（round 3 整跑）。
- **本 PR 未 push 前的状态**：分支只在远端工作树里，`master` 尚未改变；合并动作由 Leader 在用户审阅通过后执行。
