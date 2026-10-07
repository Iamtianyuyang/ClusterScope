# QA：部署与运维（O1–O21）

本维度的核心问题是「`deploy/` 里的东西和代码/配置项对不对得上」。**没有 docker 环境**，所以容器那条路只能做静态一致性检查。

前置：多数检查用 `sh qa/harness/ops-checks.sh`（自己会重启 server 一次做迁移幂等检查，结束时 server 仍在跑，记得 `server-down.sh`）。

| # | 检查项 | 操作 | 期望结果 | 判定 | 证实约束 |
|---|---|---|---|---|---|
| O1 | PostgreSQL 就绪 | `/public/tianyuyang/code/ClusterScope-review/pg16/bin/pg_ctl -D /public/tianyuyang/code/ClusterScope-review/pgdata -l /tmp/pg-server.log status; /public/tianyuyang/code/ClusterScope-review/pg16/bin/psql "$PGURL" -tAc 'select version();'` | `server is running (PID …)`；`PostgreSQL 16.4 …` | 两条都要有（`pg_start()` 会在脚本里自动拉起） | DOC-18 |
| O2 | Top CPU 进程常量 | `grep -n 'TOP_CPU_PROCESSES\|have_process_baseline' crates/agent/src/metrics.rs` | `TOP_CPU_PROCESSES: usize = 15`；首扫 `return`（不伪造 0） | PASS | DOC-20 |
| O3 | TUI 参数面 | `./target/release/clusterscope-tui --help; echo exit=$?` | exit 0，含 `--server/--username/--password/--interval` 与默认值 | PASS | DOC-17 |
| O4 | TUI 快捷键 ↔ 代码 | `sh qa/harness/doc-claims-checks.sh \| grep DOC-TUI-KEY` | 13 条全 PASS | PASS | DOC-16 |
| O5 | 采集节拍 | `sh qa/harness/ops-checks.sh \| grep -E 'AGENT-REPORT-CADENCE\|NOTE node_metrics'` | 20s 内 7–13 行（实测 10） | PASS | DOC-14 |
| O6 | 重启后重新注册 | 同脚本 `NODE-REAPPEARS-AFTER-RESTART` | ≤75s 内节点回到 `/api/nodes`（实测 47s） | PASS | DOC-15 |
| O7 | server 死配置键 | 同脚本 `grep DEADKEY` | 8 个键 0 次使用（`redis_url`/`prometheus_enabled`/`prometheus_addr`/`ws_*`×4/`tls_enabled`） | **FAIL = 结论**（其中 `redis_url` 已在 example 里注明 unused，属诚实） | DOC-11、DOC-19 |
| O8 | agent 死配置键 | `for k in log_level disk_mounts collect_process_details; do echo -n "$k: "; grep -rn "\b$k\b" crates --include='*.rs' \| grep -v common/src/config.rs \| wc -l; done` | 三个都是 0 | **FAIL = 结论**：模板里写了、代码里不读 | DOC-10 |
| O9 | 默认管理员口令 | `grep -n 'default_admin_password' crates/common/src/config.rs README.md deploy/server.yaml.example` | 文档 `admin123` / 代码默认 `admin` | **FAIL = 结论**：省略键时初始口令弱于文档 | DOC-12 |
| O10 | systemd 两套并存 | `grep -n 'systemctl --user' README.md \| head -3; grep -n 'User=\|ExecStart=' deploy/*.service` | README 用 user unit；`deploy/*.service` 是 system unit | **FAIL = 结论**：文档与 unit 文件不是同一套部署方式 | DOC-21 |
| O11 | 端口与声明 | `sh qa/harness/server-up.sh false && sh qa/harness/ops-checks.sh \| grep -E 'PORT-\|NOTE listening'; sh qa/harness/server-down.sh` | 8080/50051 在听，9090 不在（`Dockerfile.server:9` 却 EXPOSE 9090） | PASS（作为不一致的证据） | OPS-01、DOC-22 |
| O12 | Prometheus 端点 | 同脚本 `grep PROM-METRIC` | `/api/prometheus/metrics` 返回含 `nodes_total`、`nodes_online` 的文本 | PASS：指标只有 2 个，且走 REST 端口而非 9090 | OPS-02 |
| O13 | unit 里的参数有效 | `./target/release/clusterscope-agent --help \| grep -- '--config'; sh qa/harness/doc-claims-checks.sh \| grep -E 'DOC-AGENT-UNIT\|DOC-SERVER-UNIT\|DOC-INSTALL-AGENT'` | agent 用 `--config`（clap 定义存在）、server 用位置参数（argv[1] 解析存在）、install-agent.sh 用 `-c` | 全 PASS | OPS-03 |
| O14 | 部署脚本语法与路径 | `bash -n deploy/install-agent.sh && bash -n deploy/tui.sh; grep -n 'target/debug' deploy/tui.sh` | 两个脚本语法 OK；`tui.sh:17` 只找 `target/debug/clusterscope-tui` | 语法 PASS；**FAIL = 结论**：release-only 环境里 `tui.sh --install` 会失败 | OPS-04 |
| O15 | docker-compose 路径 | `command -v docker \|\| echo no-docker; command -v podman && podman images \| wc -l; sh qa/harness/doc-claims-checks.sh \| grep -E 'DOC-COMPOSE-PG16\|DOC-DOCKERFILE'` | `no-docker`；podman 镜像 0 | **N/A（本环境不可验证）**：静态项 PASS（`postgres:16-alpine`、构建上下文 `..`、env 键都被代码读取）。报告里必须写明「compose 未实测」 | OPS-05 |
| O16 | 配置模板键 ↔ serde 字段 | `sh qa/harness/doc-claims-checks.sh \| grep -E 'DOC-AGENT-YAML-KEY\|DOC-SERVER-YAML-KEY'` | 17 条全 PASS | PASS（其中 3 个 agent 键是死键，见 O8） | OPS-06 |
| O17 | 无迁移工具链 | `wc -l crates/storage/src/migrations.rs; grep -c 'CREATE TABLE IF NOT EXISTS' crates/storage/src/lib.rs` | `migrations.rs` 5 行（纯注释）；建表 9 张全内联在 `lib.rs` | **FAIL = 结论**：无 schema 版本/回滚机制，只能靠 `IF NOT EXISTS` 幂等 | OPS-07 |
| O18 | 无 root 可用性 | `sh qa/harness/auth-tui-checks.sh 2>&1 \| grep TUI-` | 4 条全 PASS（pty 里 TUI 渲染出 `ClusterScope`、节点名、快捷键提示、无 panic） | PASS | OPS-08 |
| O19 | 纯 CPU 节点降级 | `nvidia-smi --query-gpu=index --format=csv,noheader \| wc -l; grep -c 'NVML init failed' gauntlet-out/qa/agent.log` | 本机 6 张 GPU、NVML 可用（0 条 init failed） | **N/A**：README:323 承诺的「无 GPU 节点仍上报 CPU/内存」在本机**无法验证**，需要一台无卡机器；报告里标注未覆盖 | OPS-09 |
| O20 | 防火墙要求 | `ss -ltn \| grep -E ':8080\|:50051'; command -v firewall-cmd \|\| echo no-firewall-cli` | 端口在听；无 root 改不了防火墙 | **N/A（无权限）**：只能确认端口确实监听 | OPS-10 |
| O21 | TUI 只走 REST | `sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo tree -p tui --offline -e normal --depth 1' \| grep -E 'clusterscope\|common\|protocol\|storage\|server\|scheduler' \|\| echo NO-INTERNAL-CRATES` | `NO-INTERNAL-CRATES`（`crates/tui/Cargo.toml` 只有外部依赖） | PASS：架构承诺成立 | OPS-11 |

## 报告里要写明的两条部署结论

1. **部署产物与文档不是一套**：README 讲 `systemctl --user` + `~/.local/bin` + `~/.config/clusterscope`（`install-agent.sh` 那套），`deploy/*.service` 讲系统级 `User=clusterscope` + `/etc/clusterscope`。两套都能用，但必须选一套并写清。
2. **配置面有一层「幽灵旋钮」**：`prometheus_addr`、`ws_*`、`tls_enabled`、`redis_url`、`log_level`、`disk_mounts`、`collect_process_details` 共 11 个键写了不生效（其中 `tls_enabled`、`redis_url` 在文档里已注明是预留/unused）。这直接解释了 `Dockerfile.server` 的 `EXPOSE 9090` 为什么是空的。
