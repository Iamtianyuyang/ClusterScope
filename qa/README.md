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
