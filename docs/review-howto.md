# 怎么复跑这次审查（ClusterScope · GitHub 已发布线 f9c080b）

写给"明天要拿这套审查包去验一遍的人"。本页只讲**怎么跑、看到什么、结果怎么读**，
不讲实现细节；审查结论见 `report/review.html`（单文件证据包）与 `qa/qa-report.json`。

## 它能做什么（3 条）

1. **一条命令复跑全部机器闸门**：离线构建、44 个单元测试、clippy/fmt、复杂度/CRAP/覆盖率、
   重复代码、测量范围 —— 每个数字都能追溯到 `gauntlet-out/*.json`。
2. **在真实进程上验端到端行为**：起 PostgreSQL 16.4 + server + agent，跑 REST 安全矩阵、
   任务生命周期、并发/容量调度、告警去重、WebSocket 广播、保留策略。
3. **把每条结论钉在原始证据上**：81 条约束 → `qa/qa-report.json` 的逐条 verdict；
   每个 finding → `qa/evidence/` 下的原始输出文件 + 一条可复制的最小复现命令。

## 三步上手

### 0. 前置（远端 node，`tianyuyang@172.19.133.164`）

```sh
cd /public/tianyuyang/code/ClusterScope-review/gh-line
export PATH=$HOME/.cargo/bin:$PATH          # cargo 不在默认 PATH
git status --short --branch                 # 必须在 gauntlet/audit-gh-line 上、工作树干净
```

离线环境：**所有 cargo 命令都要带 `--offline`**；不要跑 `cargo fetch` / `cargo metadata`（会因缺包报错，属环境噪声）。

PostgreSQL 16.4（本次审查期间已就绪）：

```sh
/public/tianyuyang/code/ClusterScope-review/pg16/bin/pg_ctl -D /public/tianyuyang/code/ClusterScope-review/pgdata status
# 连接串：postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope
```

### 1. 机器闸门

```sh
node .gauntlet/gauntlet.mjs gate --profile quality     # 硬阈值：GATE quality: FAIL（预期，见下）
node .gauntlet/gauntlet.mjs test                       # tests 44/44 ✅；exit 1 只来自 ACCEPTANCE
```

> **别用 `| head` 截断 kit 命令**：`head` 提前关管道会让 node 收到 SIGPIPE 直接死掉，
> 而 `gate.json` / `next.md` 都是**最后才写**的——你会拿到一份陈旧报告还以为它跑完了。
> 要截断就 `> /tmp/x.log 2>&1` 再 `tail`。

### 2. 端到端（脚本自带起停，只按 PID 文件停进程）

```sh
sh qa/harness/server-up.sh false                       # 起 server（read-only 模式）
sh qa/harness/api-checks.sh                            # 20 条 REST 安全矩阵
sh qa/harness/server-down.sh

sh qa/harness/server-up.sh false
sh qa/harness/agent-up.sh qa-node-01
sh qa/harness/job-e2e.sh                               # 任务生命周期 + 告警 + WebSocket
sh qa/harness/concurrency-checks.sh                    # requeue / 容量调度 / 告警去重
sh qa/harness/ops-checks.sh                            # 保留策略 / 端口 / 死配置键
sh qa/harness/agent-down.sh
sh qa/harness/server-down.sh
```

### 3. 一键复跑全部（约 5 分钟 + 两条 long 检查约 63 分钟）

见 `qa/qa-report.json` 的 `reproduceInOneGo` 数组；8 个演示脚本可以逐个回放：

```sh
node .gauntlet/gauntlet.mjs demo demo/04-rest-security-audit-logs.json
# 录像落在 gauntlet-out/evidence/demos/<name>.{html,cast,txt}
```

## 输入 / 输出规则表

| 命令 | 正常输出 | 预期非 0 的情况 | 退出码 |
|---|---|---|---|
| `gate --profile quality` | 各闸门 ✅/❌ + `GATE quality: FAIL` | **本审查里 FAIL 是预期结论**（complexity 21/316、CRAP 45/316、覆盖率 20.7%） | 1 |
| `gate --profile full` | `❌ spec {"scenarios":0}` 后**立即停**（kit 在 spec 失败时 `break`） | 本审查不写 `features/`，所以 full 档必然停在 spec | 1 |
| `test` | `tests: 44/44 passed` | `exit 1` 只来自 `ACCEPTANCE scenarios=0` | 1 |
| `doc-claims-checks.sh` | 逐条 PASS/FAIL + 汇总 | **7 条 FAIL 是审查结论**（文档 ↔ 实现不符） | 1 |
| `api-checks.sh` | 20 条矩阵 | **2 条 FAIL 是审查结论**（`DOC-GET-USERS-READONLY`、`DOC-GET-AUDIT-LOGS`） | 1 |
| `job-e2e.sh` | `JOB-E2E: ALL PASS` | 全 PASS 才算好 | 0 |
| `concurrency-checks.sh` | 逐条 CHECK | **1 条 FAIL 是审查结论**（`CON-JOB-PID-PERSISTED`） | 1 |
| `ops-checks.sh` | 逐条 CHECK | **3 条 FAIL 是审查结论**（server `--help`、天级历史、`active_alerts`） | 1 |
| `auth-tui-checks.sh` | 14 条全 PASS | 脚本自己重启 server/agent，结束时保持停止 | 0 |
| `demo <script>.json` | 逐条 `-> exit N` + 录像路径 | 脚本里带 `expectCode` 的步骤会显示预期非 0 | 0（脚本级） |

**读法**：`qa/qa-report.json` 的 `verdict: pass` **不等于产品健康**。它的语义是
「81 条约束都被真实执行过，且每条的实际结果与清单预期一致」——其中 29 条是 `finding` 类
（**审计预期它不成立**），它们 FAIL 才是 pass。

## 在脚本或代码里怎么调用

```sh
# 只跑一个 harness 脚本，把原始输出留档
sh qa/harness/api-checks.sh > /tmp/api.txt 2>&1; echo "exit=$?"

# 复跑某一条 finding 的最小复现（F-01：审计端点恒 500）
sh qa/harness/server-up.sh false
curl -s -i http://127.0.0.1:8080/api/audit-logs | head -5     # → HTTP/1.1 500, content-length: 0
sh qa/harness/server-down.sh
```

`qa/harness/env.sh` 提供公共变量（仓库根、PG 路径、PID 文件位置），其它脚本都 `source` 它。

## 五个坑（都踩过）

1. **共享机器**：只按 PID 文件停进程（`server.pid` / `agent.pid`）。**禁止** `pkill -f clusterscope`
   ——本机有另一个与本审查无关的常驻 agent（PID 266643）。
2. **不要并发跑两条 kit 命令**：`testsGeneric` 每次开始会先删 `gauntlet-out/junit.xml`、`lcov.info`，并发会互相删报告。
3. **`features/` 是空的**：本审查不改产品代码 ⇒ 无法落地 Rust 验收测试 ⇒ `spec` / `ACCEPTANCE` 记 **N/A**。
   没有为了刷绿而写占位场景。
4. **`TUI-SHOWS-NODE` 依赖本机常驻 agent**：`qa/harness/auth-tui-checks.sh:58` 写死 `grep 'lyy-node03'`，
   换机器或那个 agent 停了这条会假 FAIL（见 N7）。
5. **`ops-checks.sh:112` 会全表清空 `node_metrics`**：在共享 PostgreSQL 上会把所有节点的原始指标删掉（见 N9）。
   跑之前先备份，或请人把这条改成只删自己的 `node_id`。

## 相关文件

| 文件 | 内容 |
|---|---|
| `report/review.html` | **单文件证据包**（16 条 findings + 闸门面板 + 合流方案 + 5 分钟审阅路线） |
| `report/merge-plan.md` | 三棵树合流方案（M1–M9 逐题） |
| `qa/qa-report.json` | 81 条约束的逐条 verdict、16 条 findings、9 条清单自我纠错 |
| `qa/evidence/` | 46 个原始证据文件 |
| `demo/*.json` | 8 个可一键回放的演示脚本 |
| `docs/architecture/clusterscope-runtime.architecture.json` | 架构图候选（Archify 校验通过） |
| `GAUNTLET.md` | 项目档案：构建/测试/运行、坑、硬阈值下的现状 |
