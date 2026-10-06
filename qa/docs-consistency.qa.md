# QA：文档 ↔ 实现一致性（D1–D22）

**规则：不得修改 `README.md` / `docs/**` / 产品代码**——不一致只记录，不在本审查里修。
凡是从文档里抠出来的声称都标了行号；凡是要对代码的都标了文件:行。

前置（静态部分不需要 server）：

```sh
cd /public/tianyuyang/code/ClusterScope-review/gh-line
sh qa/harness/doc-claims-checks.sh 2>&1 | tee gauntlet-out/qa/evidence/doc-claims.txt
```

实测结论：**该脚本 7 条 FAIL，全部是 `finding`（审查结论）**，其余全 PASS。

| # | 检查项 | 操作 | 期望结果 | 判定 | 证实约束 | 证据 |
|---|---|---|---|---|---|---|
| D1 | LICENSE 文件缺失 | `test -f LICENSE && echo LICENSE-OK \|\| echo LICENSE-MISSING; grep -n 'blob/master/LICENSE' README.md` | `LICENSE-MISSING`；README:19/363 仍指向该路径，`Cargo.toml:14` 声明 `Apache-2.0` | **FAIL = 结论**：许可证声明无对应文件（README 链接在 GitHub 上 404） | DOC-01 | `doc-claims.txt` `DOC-LICENSE-FILE` |
| D2 | 保留策略文档过时 | `grep -n '2s → 1min → 10min' docs/architecture.md; grep -n 'RAW_RETENTION_MS\|HOURLY_RETENTION_MS' crates/server/src/handlers.rs` | architecture.md:62 命中；代码是 `RAW_RETENTION_MS=24h`、`HOURLY_RETENTION_MS=7d` | **FAIL = 结论**：`docs/architecture.md:62` 与实现不符（README:305-309 与代码一致） | DOC-02 | 同上 |
| D3 | `clusterscope-server --help` | `./target/release/clusterscope-server --help; echo exit=$?` | 实际 `Error: Config file not found: --help`、`exit=1`；agent/tui 的 `--help` 正常（exit 0） | **FAIL = 结论**：依赖 clap 却手读 `argv[1]`（`crates/server/src/main.rs:182-194`） | DOC-03 | `evidence/help-server.txt` |
| D4 | 缺配置文件时的行为 | `./target/release/clusterscope-server /nonexistent/qa.yaml; echo exit=$?` | `exit=1` 且提示找不到配置 | PASS：与 `deploy/server.service` 的用法一致 | DOC-03 | `evidence/server-badcfg.txt` |
| D5 | `docs/api.md` 端点表完整性 | `sh qa/harness/doc-claims-checks.sh \| grep 'DOC-API-EP-IN-DOC'` | `/api/users/{id}`、`/api/alerts/rules/{rule_id}/state`、`/api/prometheus/metrics` 三条 FAIL（文档没写）；`/api/health` PASS | **3 条 FAIL = 结论**：文档漏了 3 个真实路由（`crates/server/src/main.rs:236-291`） | DOC-05 | `doc-claims.txt` |
| D6 | `GET /api/audit-logs` 可用性 | `sh qa/harness/server-up.sh false && sh qa/harness/api-checks.sh \| grep DOC-GET-AUDIT-LOGS; sh qa/harness/server-down.sh` | 实际 **500**（期望 200） | **FAIL = 结论**：端点不可用。原因：`audit_queries.rs:70-79` 用 `SELECT *`，模型字段是 `user`（`models.rs:142`），表列是 `username`（`storage/lib.rs:218`） | DOC-06 | `evidence/api-checks.txt` |
| D7 | api.md 认证声明 | 见 `security.qa.md#S2` | `GET /api/nodes` 无 token → 200 | **FAIL = 结论**：`docs/api.md:5`「除 health/login/refresh-token 外都要 JWT」在 read-only 模式下不成立 | DOC-04 | `evidence/api-checks.txt` |
| D8 | cluster/info 的 null 语义 | `sh qa/harness/server-up.sh false && sh qa/harness/agent-up.sh qa-node-01 && sh qa/harness/ops-checks.sh \| grep -E 'CLUSTER-INFO'; sh qa/harness/agent-down.sh; sh qa/harness/server-down.sh` | 实测 `{"idle_gpus":null,"avg_gpu_utilization":null,"active_alerts":0,...}` | `NULL-IDLE-AND-AVG` PASS；`NULL-ACTIVE-ALERTS` **FAIL = 结论**（README:358 说三者都是 null） | DOC-08 | `evidence/ops-checks.txt` |
| D9 | 「force → SIGKILL」承诺 | `grep -rn 'SIGKILL' crates/ \| wc -l; grep -rn '"force"' crates/ \| wc -l` | 两个都是 **0** | **FAIL = 结论**：README:355 描述的升级机制不存在（代码只有 SIGTERM，`job_executor.rs:33-48`） | DOC-09 | `doc-claims.txt` |
| D10 | agent 配置模板的键是否生效 | `for k in log_level disk_mounts collect_process_details; do echo -n "$k: "; grep -rn "\b$k\b" crates --include='*.rs' \| grep -v common/src/config.rs \| wc -l; done` | 三个都是 **0** | **FAIL = 结论**：`deploy/agent.yaml.example:20,23,28` 与 README:246 文档化的键被静默忽略（`collect_disks` 用的是 sysinfo 全盘列表，`metrics.rs:146-170`） | DOC-10 | `evidence/ops-checks.txt` `DEADKEY` 行 |
| D11 | server 死配置键 | `sh qa/harness/server-up.sh false && sh qa/harness/ops-checks.sh \| grep DEADKEY; sh qa/harness/server-down.sh` | `redis_url`、`prometheus_enabled`、`prometheus_addr`、`ws_heartbeat_interval_secs`、`ws_slow_threshold_ms`、`ws_max_backlog`、`max_concurrent_ws_clients`、`tls_enabled` 都是 0 次使用 | **FAIL = 结论**（`redis_url` 在 `server.yaml.example:9` 已注明 unused，属诚实；其余未声明） | DOC-11、DOC-19 | 同上 |
| D12 | 默认管理员口令 | `grep -n 'default_admin_password' crates/common/src/config.rs README.md deploy/server.yaml.example` | README:232 与 example:13 写 `admin123`；`config.rs:120` 默认 `admin` | **FAIL = 结论**：省略该键时初始口令与文档不同（危险默认） | DOC-12 | `doc-claims.txt` |
| D13 | Web 前端「已移除」的边界 | 见 `concurrency.qa.md#C14` | `/ws` 仍在注册（`main.rs:239`）且 WS 实测可用 | **FAIL = 结论**：README:356 的「Web 前端已移除」容易被读成 WebSocket 也没了；api.md:86-95 仍文档化它 | DOC-13 | `evidence/ws-check.txt` |
| D14 | 指标节拍 2s | 见 `deploy-ops.qa.md#O5` | 20s 内新增 10 行 | PASS | DOC-14 | `evidence/ops-checks.txt` |
| D15 | 重启后 60s 内重新注册 | 见 `deploy-ops.qa.md#O6` | 实测 47s | PASS | DOC-15 | 同上 |
| D16 | TUI 快捷键表 | `sh qa/harness/doc-claims-checks.sh \| grep 'DOC-TUI-KEY'` | 13 条全 PASS（`crates/tui/src/main.rs:85-140`） | PASS | DOC-16 | `doc-claims.txt` |
| D17 | TUI 参数与默认值 | `./target/release/clusterscope-tui --help; sh qa/harness/doc-claims-checks.sh \| grep 'DOC-TUI-FLAG'` | `--server/--username/--password/--interval` 齐备，默认 `http://127.0.0.1:8080`、`3` | PASS | DOC-17 | `evidence/help-tui.txt` |
| D18 | PostgreSQL v16+ | `.../pg16/bin/psql "$PGURL" -tAc 'select version();'` | `PostgreSQL 16.4 …`；`docker-compose.yml:5` 用 `postgres:16-alpine` | PASS | DOC-18 | `doc-claims.txt` |
| D19 | TLS 只是预留 | `sh qa/harness/ops-checks.sh \| grep 'DEADKEY tls_enabled'` | 0 次使用 | PASS：与 README:353「tls_enabled 已预留」一致 | DOC-19 | `evidence/ops-checks.txt` |
| D20 | Top CPU 进程 = 15 | `grep -n 'TOP_CPU_PROCESSES\|have_process_baseline' crates/agent/src/metrics.rs` | `TOP_CPU_PROCESSES: usize = 15`；首扫只建基线不产出 | PASS：与 README:166,219 一致 | DOC-20 | `doc-claims.txt` |
| D21 | systemd 用户级 vs 系统级 | `grep -n 'systemctl --user' README.md \| head -3; grep -n 'User=\|ExecStart=' deploy/*.service` | README:288-291 文档化 `systemctl --user`；unit 是系统级（`User=clusterscope`、`/usr/local/bin`、`/etc/clusterscope`） | **FAIL = 结论**：两套部署方式并存且 README 未说明差异 | DOC-21 | `evidence/doc-claims.txt` |
| D22 | Dockerfile 端口声明 | `sh qa/harness/ops-checks.sh \| grep -E 'PORT-9090\|NOTE listening'` | `EXPOSE 8081 9090` 但实际只监听 8080/50051 | **FAIL = 结论**（9090 端口声明是死配置的镜像） | DOC-22 | `evidence/ops-checks.txt` |

## 备注

- 每条检查的「期望结果」如果是**观测到的缺陷行为**，判定列会写「FAIL = 结论」——这是审查证据，不是待修项（本审查不派编码阶段）。
- D2/D12/D21 属于「文档过时或默认值危险」，建议进合流方案时一并处理（见 `merge-plan-requirements.md` M8）。
