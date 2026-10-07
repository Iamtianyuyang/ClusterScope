# ClusterScope 审查 QA 包（第 1 阶段产出）

本目录是**审查清单 + 执行程序**，供第 5 阶段（QA）逐条真跑、第 6 阶段（报告）取证。
审查对象是 **GitHub 已发布线**：`f9c080b`（= 本仓库 `gauntlet/audit-gh-line` 的工作树内容，TUI-only，无 `web/`）。

## 怎么读

| 文件 | 内容 |
|---|---|
| `constraints.json` | 可机器检查的约束清单（每条：断言 / 依据 / 检查命令 / 期望 / 判定）。**审查结论的骨架** |
| `build-gates.qa.md` | G1–G12：真实构建/测试/静态闸门与覆盖率机制的复现 |
| `docs-consistency.qa.md` | D1–D22：README / `docs/api.md` / `docs/architecture.md` 每条可验证声称 ↔ 代码 |
| `security.qa.md` | S1–S17：JWT、refresh 轮换、锁定、read-only 边界、agent_token、注入面、审计 |
| `concurrency.qa.md` | C1–C16：任务生命周期、requeue、容量调度、告警去重、保留策略、WS 广播、迁移幂等 |
| `deploy-ops.qa.md` | O1–O21：`deploy/` 与代码/配置项的对应、端口、TUI 冒烟、N/A 项说明 |
| `merge-plan-requirements.md` | M1–M9：三棵树合流方案的**必答问题**（第 6 阶段的输入） |
| `harness/*.sh` | 已在远端**跑通**的执行脚本（下面「一键复跑」） |

**七个维度里的两个 N/A**（用户边界 + 环境限制，报告里照实写）：

- **前端**：本线是 TUI-only，`f9c080b` 已删除 `web/`（无 `web/`、无 `package.json`、无 html/ts/js）。前端只在 A/B 两棵树里（`ClusterScope-review/local-wip/web/`，27 个文件），因此前端审查 **N/A**，只在合流方案里作为「另一棵树的工作」处理（见 `merge-plan-requirements.md` M2/M3/M5，约束 `FE-01`）。
- **容器部署**：本机没有 `docker`（podman 零镜像、无外网），`deploy/docker-compose.yml` 与 `Dockerfile.server` 只能做静态一致性检查（约束 `OPS-05`，检查 O15）。

**判定口径**：约束分三类 —— `must-hold`（产品承诺，应当 PASS）、`finding`（**审计预期它不成立**，FAIL 就是审查结论）、`long`/`na`（耗时长或本环境不可验证，文档里写明理由）。
第 5 阶段的 `qa/qa-report.json` 每条检查请写 `"constraint": "<id>"`，第 6 阶段的约束闸门按这个字段配对。

## 环境（远端 node，`tianyuyang@172.19.133.164`）

- 仓库：`/public/tianyuyang/code/ClusterScope-review/gh-line`，分支 `gauntlet/audit-gh-line`
- Rust 1.97.1（`cargo` 不在默认 PATH，脚本里已 `export PATH=$HOME/.cargo/bin:$PATH`）；**无外网**，一律 `--offline`
- PostgreSQL 16.4 已就绪：`postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`
  （`psql`/`pg_ctl` 不在 PATH，用 `/public/tianyuyang/code/ClusterScope-review/pg16/bin/`）
- 本机有 **6 张 NVIDIA L20**，`nvidia-smi` 可用，agent 能上报真实 GPU 指标（NVML 初始化成功）
- 工具：`curl` `jq` `python3`(3.9) `node`(v26，自带全局 WebSocket) `script` `timeout` `ss` `openssl` `nc`
- **没有** `docker`（podman 零镜像），所以 compose 那条路不可验证（见 O15）

## 一键复跑（脚本都已实测通过）

```sh
cd /public/tianyuyang/code/ClusterScope-review/gh-line

# 1) 静态文档一致性（不需要 server）
sh qa/harness/doc-claims-checks.sh                 # 预期：7 条 FAIL，全部是 finding

# 2) 真实闸门（离线）
node .gauntlet/gauntlet.mjs gate --profile quality  # 预期：GATE quality: FAIL（complexity/crap/coverage）
node .gauntlet/gauntlet.mjs test                    # 预期：tests 44/44 ✅；ACCEPTANCE FAIL（features/ 为空）

# 3) REST 安全矩阵（read-only 模式）
sh qa/harness/server-up.sh false
sh qa/harness/api-checks.sh                        # 预期：2 条 FAIL（DOC-GET-USERS-READONLY / DOC-GET-AUDIT-LOGS）
sh qa/harness/server-down.sh

# 4) 端到端：任务生命周期 + 告警 + WebSocket（需要 agent）
sh qa/harness/server-up.sh false
sh qa/harness/agent-up.sh qa-node-01
sh qa/harness/job-e2e.sh                           # 预期：全 PASS
sh qa/harness/concurrency-checks.sh                # 预期：1 条 FAIL（CON-JOB-PID-PERSISTED，finding）
sh qa/harness/ops-checks.sh                        # 预期：3 条 FAIL（server --help / daily tier / active_alerts null）
sh qa/harness/agent-down.sh
sh qa/harness/server-down.sh

# 5) auth_required:true + agent_token + TUI 渲染（脚本自己重启 server/agent，结束时保持停止）
sh qa/harness/auth-tui-checks.sh                   # 预期：全 PASS
```

证据默认落在 `gauntlet-out/qa/evidence/`（已 `.gitignore`）。**跑完请把每条检查的 pass/fail 与原始输出写进 `qa/qa-report.json` 并提交。**

## 三条硬规矩（脚本已遵守，人工复跑时也请遵守）

1. **只按 PID 文件停进程**（`server.pid` / `agent.pid`）：这台机器是共享的，`pkill -f clusterscope` 会误杀别人的进程。
2. **不要用 `| head` 截断 kit 命令**：SIGPIPE 会让 node 提前死掉，而报告是最后才写的（GAUNTLET.md 坑 #7）。
3. **不要并发跑两条 kit 命令**：它们会互相删 `gauntlet-out/junit.xml` / `lcov.info`。

## 本轮（no-root 修复）复跑 —— 分支 `gauntlet/no-root-fixes`

本分支修四处 no-root 缺陷（`NF-01` / `NF-02` + 随仓库缺用户级 unit + 无 root 的 PG 路径未文档化），
QA 入口是**新增的** `qa/no-root-fixes.qa.md`（F1–F12）：

```sh
cd /public/tianyuyang/code/ClusterScope-review/nr-fixes
sh qa/harness/no-root-fixes-checks.sh              # 全量：含 release 构建 + systemd 真装真启 + 还原
sh qa/harness/no-root-fixes-checks.sh --no-slow    # 跳过 F1 动态探针与 F5 的 systemd 起停

node .gauntlet/gauntlet.mjs gate --profile specifier   # 第 1 阶段：预期 PASS（1 feature / 6 scenario）
node .gauntlet/gauntlet.mjs test                       # 第 2 阶段起：tests 全绿 + ACCEPTANCE 6/6
```

规格阶段（2026-10-07、**未修复**的代码）实跑：`PASS=3 FAIL=9`，退出码 9 —— F1–F9 FAIL（逐条对应四处缺陷）、
F10–F12 PASS。**F1 有安全阀**：脚本里还有「用字面量名字整机匹配」的 `pkill`/`killall` 时只做静态判定、
**不执行**动态探针（否则会在这台共享机器上误杀别人正在跑的 agent）。

返工（2026-10-07、`[spec]`）：F12 的允许集加入 `gauntlet-tools/*` 并新增 5 条负例自检、F11 的期望条数
117→118（新增 `FIX-14`，口径未动）——改动、理由与复核步骤见 `qa/no-root-fixes.qa.md#工具修复复核`。
返工后重跑：**`PASS=12 FAIL=0`，退出码 0**（证据 `gauntlet-out/qa/evidence/no-root-fixes-checks.txt`）。

本轮 14 条验收约束是 `qa/constraints.json` 里的 **`FIX-01`…`FIX-14`**（全部 `must-hold`；`FIX-14` = 两处流水线
工具修复的准入记录，独立复核步骤见 `qa/no-root-fixes.qa.md#工具修复复核`）；判据、期望与证据落点
见 `qa/no-root-fixes.qa.md`。

> 上面「一键复跑」的 1)–5) 是**审查分支 `gh-line`** 的入口；本分支（`nr-fixes`）用本节这两条。

## 第 5 阶段（QA）在 `gauntlet/no-root-fixes` 上的复核 —— 已提交

第 5 阶段在这一层上做的不是「再跑一遍作者的脚本」，而是**独立复核**（作者自述一律不信）：

```sh
cd /public/tianyuyang/code/ClusterScope-review/nr-fixes
sh qa/harness/no-root-fixes-checks.sh      # F1–F12：干净状态复跑，PASS=12 FAIL=0
node .gauntlet/gauntlet.mjs gate --profile coder   # spec/build/tests/acceptance 全绿，59/59 + 6/6
node .gauntlet/gauntlet.mjs demo demo/11-no-root-fixes-install-agent-safety.json
node .gauntlet/gauntlet.mjs demo demo/12-no-root-fixes-user-units-and-config-errors.json
```

配套材料（全部提交在分支上）：

| 文件 | 内容 |
|---|---|
| `qa/qa-report.json` | 追加 `Q401`…`Q414`（`FIX-01`…`FIX-14` 的配对条目）与 `Q421`…`Q431`（受影响既有约束 `NR-01/03/05/06/06b/08/09/10/12/13/14/15/19/21` 的回归复跑）。既有 106 条检查一字未动 |
| `qa/evidence/no-root-fixes-FIX14-tool-fix-recheck.txt` | 两处流水线工具修复的四项独立复核（产物分类 / 测试数对账 / F5 断言集 diff / 允许集负例与正例） |
| `qa/evidence/no-root-fixes-phase5-*` | 回归复跑、正常路径端到端、F1 静态闸门的负例对照、`gate --profile coder` 复跑、两个 demo 的实跑记录 |
| `demo/11-*.json`、`demo/12-*.json` | 两个可回放演示（安装脚本安全 / 用户级 unit + 配置报错语义） |

第 5 阶段记录的两条偏差（见 `qa/qa-report.json#findings` 的 `F-17`/`F-18`）：
F5 的 agent unit「真装真启」在**同名服务已在跑**的机器上是 no-op（只能证明 MainPID 不变，不能证明新起成功）；
`NR-06` 的旧文案（「`-c` 缺失应静默回退继续跑」）与本轮修复后的硬错误语义字面相反——两者都未修，等 Leader/用户裁决。

### 一条要请 Leader 裁决的范围口径缺口（第 5 阶段结尾记录）

在本分支的干净 HEAD 上跑 `sh qa/harness/no-root-fixes-checks.sh`，结果是 **F1–F11 PASS、F12 FAIL**
（`PASS=11 FAIL=1`）。F12 的失败项是第 5 阶段按任务书第 6 项新增的两个演示脚本
（`demo/11-no-root-fixes-install-agent-safety.json`、`demo/12-no-root-fixes-user-units-and-config-errors.json`）——
F12 的允许集（`in_scope()`）写于第 1 阶段，里面没有 `demo/`。**这不是四处修复的缺陷**：本阶段开工时的干净状态
是 `PASS=12 FAIL=0`，`demo/` 之外的一切判定都没变。

两个候选修法（都需要批准，本轮没有自己动手，见 `qa/qa-report.json#findings` 的 `F-20` 与检查 `Q432`）：

1. 在 `qa/harness/no-root-fixes-checks.sh` 的 `in_scope()` 里加 `demo/*`（1 行；5 条负例自检与反向控制仍是 5/5），
   **并把该修订以追加一条约束的形式记录到 `qa/constraints.json`** —— 注意追加会动到数组末条（补逗号），
   会命中 F11 的『零删行』口径，所以要走明确裁决而不是由阶段子 agent 自行落笔；
2. 接受 F12 的这个 FAIL 作为解释性结论，由第 6 阶段在证据包里写明「新交付物 vs 旧范围清单」。

证据：`qa/evidence/no-root-fixes-FIX15-scope-amendment.txt`（三种状态 A/B/C 的原始输出）、
`qa/evidence/no-root-fixes-checks-rerun-raw.txt`（干净状态 F12 PASS 的原文）。

**2026-10-07 裁决（已落地）**：Leader 采纳**候选 1**，执行口径是「只改 harness 的一行 + 注释里写理由，
**不**动 `qa/constraints.json`（避开 `F11` 的『零删行』口径）」。落笔在第 1 阶段（`[spec]`）：

- `qa/harness/no-root-fixes-checks.sh` 的 `in_scope()` 允许集加入 `demo/*`（该行上方注释写明理由与裁决日期）；
- F12 另加 **1 条正例自检**（`demo/*` 必须判**允许**），5 条负例自检与 `5/5` 计数**原样不变**；
- `qa/constraints.json` **一字未动**（既有 104 条与 `FIX-01`…`FIX-14` 的口径不变，`F11` 继续原样有效）。

复跑结果：**`PASS=12 FAIL=0`（退出码 0）** —— 上一节记录的 F12 FAIL 已消解，四处修复本身未被改动。
证据：`qa/evidence/no-root-fixes-FIX15-scope-amendment.txt` 状态 C（复跑全文）、
`qa/evidence/no-root-fixes-FIX15-F12-scope-after-amendment.txt`（F12 段的改动文件清单 + 正/负例自检）。

## 本轮（M6 三线合流）—— 分支 `gauntlet/merge-m6`

本分支把 **B 线**（`b/master` = `19d8fbc`，12 个未提交文件 + 修 F-01/F-16 的 `eac070e`）与 **A 线**
（`../local-wip/`：`common/src/metrics.rs`、`storage/src/conversions.rs`、`tests/integration_test.rs`）合流进主线
（基线 `master` @ `8601ac9`）。**M5 裁决 = 先 TUI-only**，所以 `web/` 与 `deploy/nginx.conf` 本轮不做；
**M7 质量口径不动**（97 项欠账是已记录的审查结论，不还债、不重算成 PASS）。

| 文件 | 内容 |
|---|---|
| `qa/merge-m6.qa.md` | **本轮的执行程序与判据**：合流前基线表、M6 步骤 1/2/3/5 的逐条检查、M9 行为等价清单、M10 不变量（NRM1–NRM8）、**PASS/FAIL 翻转登记表**、F12 的轮次口径、F-02/F-07/F-12 的取舍记录 |
| `features/merge_m6_*.feature` | 22 个新验收场景（审计查询 / 登录限速与令牌吊销 / 任务参数上限与 SIGKILL 升级与配置键 / A 的指标类型），场景名一律 snake_case |
| `qa/harness/merge-m6-checks.sh` | 判据程序 **M6-01…M6-15**（`--static` 只跑不需要 server/agent 的 10 段） |
| `qa/constraints.json` | 追加 **`MRG6-01`…`MRG6-20`**（`must-hold`）；既有 118 条逐字节未动，合计 138 条，追加是纯插入（`--numstat` 第二列为 0） |

```sh
cd /public/tianyuyang/code/ClusterScope-review/merge-m6
sh qa/harness/merge-m6-checks.sh --static      # 合流前：10 PASS / 1 FAIL（唯一 FAIL 是等合流来修的 M6-08）
sh qa/harness/merge-m6-checks.sh               # 合流后：要求 0 FAIL
sh qa/harness/no-root-fixes-checks.sh --no-slow # 四项无 root 修复 12/12（M6 轮口径）
```

**两条与本轮同时落地的 harness 修正**（沿用 `FIX-14`/`FIX-15` 的先例，断言集合一行未动，**需 Leader 追认**）：

1. **树定位**：`env.sh` / `no-root-checks.sh` / `nr-verify*.sh` 原来把仓库根硬编码成 `.../gh-line` —— 在
   `merge-m6` 里跑会静默地测**旧树**（假证据）。现在按脚本位置自解析，仍可用 `REPO=` / `R=` 覆盖（约束 `MRG6-16`）。
2. **F12 的轮次口径**：`F12` 是「范围 vs 某一轮基线」的检查，而合流注定要改 `crates/server/**`、`crates/storage/**`
   （在 no-root 轮的允许集里是越界）。现在按轮次选允许集：no-root 轮旧口径可用 `NR_FIX_BASE=7ca587a` 复跑，
   M6 轮用 `m6_in_scope()`（另配 5 条负例自检 + 2 条正例自检）。**修脚本前的实测是 11/12**（F12 唯一失败项是
   旧允许集没有 `report/*`），详见 `qa/merge-m6.qa.md` §8（约束 `MRG6-10`）。
