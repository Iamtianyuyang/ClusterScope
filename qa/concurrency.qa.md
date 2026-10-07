# QA：并发与正确性（C1–C16）

前置（与 `security.qa.md` 相同）：

```sh
cd /public/tianyuyang/code/ClusterScope-review/gh-line
sh qa/harness/server-up.sh false
sh qa/harness/agent-up.sh qa-node-01     # 节点 gpu_count=6（真实 L20）
sh qa/harness/job-e2e.sh                 # C1–C3、C14
sh qa/harness/concurrency-checks.sh      # C4–C8
sh qa/harness/ops-checks.sh              # C9、C12、C13
sh qa/harness/agent-down.sh
sh qa/harness/server-down.sh
```

时间预算（实测）：`job-e2e.sh` ≈ 90s，`concurrency-checks.sh` ≈ 2min（含 25s 长任务与容量等待），`ops-checks.sh` ≈ 3min（含重启后 47s 等重注册 + 20s 节拍 + 20s 保留）。

| # | 检查项 | 操作 | 期望结果 | 判定 | 证实约束 |
|---|---|---|---|---|---|
| C1 | 任务生命周期 | `sh qa/harness/job-e2e.sh \| grep -E 'JOB-SUCCEEDED\|JOB-LOGS-CAPTURED'` | 短任务 `/bin/sh -c "echo qa-hello; sleep 1; echo qa-done"` → `succeeded`；`/api/jobs/{id}/logs` 里两条输出都在 | 两条 PASS | CON-01 |
| C2 | 取消真的杀进程组 | 同脚本 `JOB-RUNNING`/`JOB-CANCELLED`/`JOB-PROC-GONE` | `sleep 300` 任务 DELETE 后 `cancelled`，且 `pgrep -f 'sleep 300'` 计数为 0 | 三条 PASS（无孤儿进程） | CON-02 |
| C3 | 终态/未知任务停止 | 同脚本 `JOB-STOP-TERMINAL-409`/`JOB-STOP-UNKNOWN-404` | 409 / 404 | 两条 PASS | CON-03 |
| C4 | 卡住的 starting 会被 requeue | `sh qa/harness/concurrency-checks.sh \| grep CON-REQUEUE` | 直插一行 `status='starting'`、`started_at=now()-30min`、节点 `qa-ghost-node`（在 node_info 里但**不在内存 registry**）→ ≤30s 变 `queued` 且 `started_at IS NULL` | 两条 PASS（实测 10s 内） | CON-04 |
| C5 | GPU 容量感知调度 | 同脚本 `CON-CAPACITY-*` | 6 卡节点上两个 `resource_quota:"gpu:6"`：第一个 `running`、第二个 `queued`；取消第一个后第二个被派发 | 三条 PASS | CON-05 |
| C6 | 告警去重 | 同脚本 `CON-ALERT-*` | 规则 `load_1 gte 0 duration 0` 命中 → 出现 `firing` 事件；10s 内 5 次上报后事件数**不变** | 三条 PASS | CON-06 |
| C7 | 任务 pid 是否落库 | 同脚本 `CON-JOB-PID-PERSISTED` + `NOTE jobs rows` | 实测：10 行任务、**0 行有 pid**，而 agent 日志有 7 条 `Process spawned` | **FAIL = 结论**：`grpc.rs:497-506` 调 `update_job_status` 时 pid 恒为 `None`，UI 看不到 pid | CON-07 |
| C8 | 重试机制 | 同脚本 `CON-RETRY-NEVER-USED`；`grep -rn 'retry_count' crates/server/src/*.rs \| grep -v 'retry_count: 0'` | 0 行非零；代码里只有字面 `0` | PASS（**同时是结论**：`retry_count/max_retries` 是死列，失败任务永不重试） | CON-08 |
| C9 | 原始指标保留 24h | `sh qa/harness/ops-checks.sh \| grep RETENTION-RAW` | 插 25h 前的行 + 1h 前的行 → 20s 后旧行被删、新行保留 | 两条 PASS（后台 tick 10s） | CON-09 |
| C10 | 小时/天/日志保留的节拍 | 长时间检查（**≥10 分钟**）：`sh qa/harness/server-up.sh false; sh qa/harness/agent-up.sh qa-node-01; psql_q "insert into metrics_hourly ... hour_bucket = now() - interval '8 days';"; psql_q "insert into job_logs ... timestamp = now() - interval '31 days';"; sleep 620; psql_q "select count(*) from metrics_hourly where hour_bucket < now() - interval '7 days';"; psql_q "select count(*) from job_logs where timestamp < now() - interval '30 days';"` | 两处都是 0 | 两条都 0 → PASS。清理挂在 `cycle % 60 == 0`（10 分钟）与 `cycle % 360 == 0`（1 小时）上，**最坏延迟 10 分钟**——报告里要写明这个口径 | CON-10 |
| C11 | 小时聚合的幂等与延迟 | 长时间检查（**≥10 分钟**）：起 server+agent 后 `sleep 620`，再 `psql_q "select count(*) from metrics_hourly where node_id='qa-node-01';"` | ≥1（当小时桶）；连跑两次 tick 后行数不翻倍（`UNIQUE(node_id,metric_name,hour_bucket)` + `ON CONFLICT DO UPDATE`） | PASS；同样受 10 分钟 tick 限制 | CON-11 |
| C12 | 三档历史合并 | `sh qa/harness/ops-checks.sh \| grep HISTORY-` | `HISTORY-HOURLY-SOURCE` PASS（3 天前的桶带 `source:"hourly"`）、`HISTORY-SORTED` PASS、`HISTORY-MISSING-PARAMS-400` PASS；`HISTORY-DAILY-SOURCE` **FAIL** | 3 PASS + 1 FAIL：**天级（90 天）永不返回**。DB 里确有 4 行命中同一 SQL 过滤条件（`qa/harness/diag-daily.sql` 可复现），说明是 Rust 侧把 `DATE` 列解码成 `chrono::DateTime<Utc>` 失败，而 `handlers.rs:274-285` 用 `if let Ok(...)` 把错误吞了 | CON-12、DOC-07 |
| C13 | 迁移幂等 | `sh qa/harness/ops-checks.sh \| grep MIGRATION-` | 重启后 `/api/health` 200、`users` 里 admin 计数仍为 1 | 两条 PASS | CON-13 |
| C14 | WebSocket 广播 | `sh qa/harness/job-e2e.sh \| grep WS-` | `connected/subscribed` 都收到；15–25s 窗口内 `metrics_update` ≥1、`job_update` ≥1（实测 metrics=13、jobs=3、alerts=2） | 四条 PASS | CON-14、DOC-13 |
| C15 | gRPC 流不泄漏 | `sh qa/harness/agent-down.sh; sleep 12; grep -c 'get_jobs_for_node failed' gauntlet-out/qa/server.log` | agent 断开后 `get_pending_jobs` 的 5s 轮询因 `tx.send().is_err() → return` 退出（`grpc.rs:347`），日志不再增长 | PASS（静态 + 观察）；`report_metrics` 每次上报一条独立短流（`grpc_client.rs:128-140`） | CON-15 |
| C16 | 去重缓存有界 | `grep -n 'LruCache::new' crates/server/src/main.rs` | `NonZeroUsize::new(100000)`，无 TTL | PASS：不会无界增长；重启即清空（配合 `sequence` 用挂钟毫秒播种，`grpc_client.rs:53,129-134`，避免重启后误判重复） | CON-16 |

## 双跑窗口（写报告时要讲清的机制）

`main.rs:399-435` 的 requeue **只在「节点已不在线」时**才把 `starting` 改回 `queued`，并且用 `WHERE status='starting'` 做状态守卫（`job_queries.rs:202-215`）；agent 侧用 `pids` 里的占位 pid 防重复 spawn（`grpc_client.rs:214-235`）、`job_executor.rs:161-166` 处理「取消与 spawn 竞争」。
**实测覆盖**：C4 证明 requeue 生效；C2 证明取消不会留孤儿。**未覆盖**：真正「慢 agent + 双派发」的竞态只能靠代码阅读（本审查不改代码，无法构造）。
