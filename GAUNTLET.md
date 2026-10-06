# GAUNTLET.md — 项目档案

commit: `f9c080b`　base: `master`　更新：2026-10-07　适配器：commands　棘轮：开（基线 77 项）

> 本档案记录的是**实际跑通过**的命令和真实测量值。未验证的内容一律标注「未验证」。
> 闸门命令的运行位置：远端 node（Linux，`tianyuyang@172.19.133.164`），仓库
> `/public/tianyuyang/code/ClusterScope-review/gh-line`，分支 `gauntlet/audit-gh-line`。

## 一句话

ClusterScope —— 轻量级 Linux GPU 集群监控平台（普通用户即可运行，无需 root）。交付物是 **3 个 Linux x86_64 二进制**：

- `clusterscope-agent`：部署在每台 GPU 节点，NVML + `/proc` 采集指标，gRPC 上报，并执行被派发的任务
- `clusterscope-server`：中央服务，gRPC(:50051) + REST(:8080) + 调度 + 告警，持久化到 **PostgreSQL**
- `clusterscope-tui`：ratatui 终端仪表盘，只通过 REST 读 server（不链接任何内部 crate）

Rust workspace（Cargo，resolver 2，edition 2024），7 个成员 crate，`[workspace.package] version = "0.1.1"`。

## 构建、测试、运行（已实际运行）

| 做什么 | 命令 | 耗时 | 备注 |
|---|---|---|---|
| 构建（debug，含全部 target） | `node .gauntlet/gauntlet.mjs build`（= `commands.build`：`sh -c "export PATH=$HOME/.cargo/bin:$PATH; cargo build --workspace --all-targets --offline"`） | 全新 target 目录 23s；增量 <1s | 128 核；2.4G 产物 |
| 发布构建 | `cargo build --release --workspace --offline` | 28s | 产物 `target/release/clusterscope-{agent,server,tui}` = 5.9M / 12.4M / 8.1M |
| 全部测试（闸门用，含覆盖率与 JUnit） | `node .gauntlet/gauntlet.mjs test` → `node gauntlet-tools/rust-gate.mjs --out gauntlet-out` | 41s（含插桩编译 20s），增量复跑约 10s | 44 通过 / 0 失败 / 0 忽略 |
| 全部测试（不带覆盖率，快路径） | `cargo test --workspace --offline` | 3s（热） | 与上面同一批 44 个测试 |
| 静态检查 | `cargo clippy --workspace --all-targets --offline` / `cargo fmt --all --check` | 5s / <1s | 两者都 0 发现 |
| 全量质量闸门 | `node .gauntlet/gauntlet.mjs gate --profile quality` | 19s（热） | 见「质量现状」 |
| 运行交付物（冒烟） | `./target/release/clusterscope-agent --help`、`clusterscope-tui --help` | — | 两者 exit 0 并打印帮助 |
| 运行 server | `./target/release/clusterscope-server <config.yaml>` | — | 需要 PostgreSQL 16+；**不支持 `--help`**（见「审查线索」） |

要让服务器端到端跑起来需要 PostgreSQL 16+（`deploy/docker-compose.yml` 可起 postgres + server），本机/远端都**没有** postgres、docker 镜像（podman 在但无镜像、无外网）——这是第 5 阶段 QA 的前置阻塞项，见「坑」第 7 条。

## 代码地图

| crate | 职责 | 依赖的内部 crate |
|---|---|---|
| `crates/common` | 共享类型、配置、告警状态机、任务状态机、JWT/argon2 认证工具、节点注册表 | 无（叶子） |
| `crates/protocol` | `proto/clusterscope.proto` 的 gRPC 生成代码（`build.rs` 用 tonic-build 编译，**需要 protoc**） | 无（叶子） |
| `crates/storage` | PostgreSQL 访问层（sqlx）：各表查询、三档聚合与清理 | common |
| `crates/scheduler` | GPU 容量感知的 FIFO 调度（单文件 426 行） | common |
| `crates/agent` | 节点采集器：NVML/`/proc` 指标、任务执行器、gRPC 客户端、配置加载 | common, protocol |
| `crates/server` | 中央服务：`main.rs` 组装 axum REST + tonic gRPC + 后台循环；`handlers.rs` REST；`grpc.rs` gRPC 服务；`auth_middleware.rs` JWT/只读中间件；`ws_handler.rs` WebSocket 广播 | common, protocol, storage, scheduler |
| `crates/tui` | 终端仪表盘：`ui.rs`（1566 行，最大文件）绘制，`api.rs` REST 客户端 | 无（只走 REST） |

- 依赖方向一句话：`common`/`protocol` 是叶子 → `storage`/`scheduler`/`agent` 只依赖叶子 → `server` 依赖全部四个 → `tui` 独立。
- 入口（谦卑对象）：三个 bin 的 `src/main.rs`。注意 `crates/server/src/main.rs` 不是纯转发：524 行里组装 REST/gRPC + 后台循环（`run_background_tasks`、`run_scheduler_cycle`），其中 331 行未被任何测试加载（0% 覆盖）。
- 测试：**全部**是文件内 `#[cfg(test)] mod tests`（8 个文件），没有 `crates/*/tests/` 集成测试目录，没有 `tests/` 顶层目录。
- 产品代码合计：33 个 `.rs`，10,104 行；闸门统计的「代码行」7,030 行、316 个函数。

## 现有规矩

- README「测试」节写的三条本地闸门（我逐条跑过，当前全绿）：
  `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace`。
- **没有 CI**：`.github/` 不存在，`survey` 报 `CI: 0 file(s)`。上面三条命令目前只能人工手动跑，没有任何自动化闸门——这是本次审查的重点之一。
- 没有任何质量工具配置文件（无 `rustfmt.toml` / `clippy.toml` / `deny.toml` / `rust-toolchain.toml`），一律用工具默认规则；代码当前 clippy + rustfmt 全干净。
- 依赖统一走 `[workspace.dependencies]` + `xxx.workspace = true`；只有少数直接写版本（`sysinfo`、`libc`、`nvml-wrapper`、`ratatui`、`crossterm`、`argon2`、`tokio-stream`、`serde_yaml`）。
- 依赖规模：`Cargo.lock` 412 个包；远端 crates.io 缓存 501 个 Linux 包（离线可用）。
- 提交信息用 `feat:` / `fix:` / `docs:` / `chore:` 前缀。仓库历史 40 个 commit、3 个作者、首提交 2026-08-10、HEAD 2026-08-14。

## 质量现状（基线，commit f9c080b，`gate --profile quality`，棘轮关）

| 闸门 | 结果 | 数字 |
|---|---|---|
| build | ✅ | — |
| tests | ✅ | 44/44 通过，0 失败，0 忽略；doc test 0 个 |
| scope | ✅ | 33/33 文件被内置分析器解析（`engine=builtin`，lizard 未装） |
| complexity | ❌ | 316 个函数中 **21 个超标**，最大圈复杂度 **23**（阈值 10） |
| warnings | ✅ | 0（`clippy --workspace --all-targets`，含 `-D warnings` 变体） |
| tidy | ✅ | 0 发现（`cargo fmt --all --check` 干净） |
| duplication | ✅ | 0.0%（7030 代码行中 0 处克隆，min 100 tokens） |
| crap | ❌ | **45 个函数超标**，最大 CRAP **552**（阈值 8） |
| arch | ➖ 跳过 | 未配置 `commands.arch`（远端没有 cargo-modules/cargo-deny，装不了） |
| coverage | ❌ | 行覆盖 **20.7%**（1381/6686 行；阈值 90%） |

最严重的几个函数（`gauntlet-out/static.json`、`crap.json`）：`tui/src/ui.rs:599 node_panel`（cc=23、170 行、CRAP 552）、`tui/src/ui.rs:1024 draw_process`（cc=18、196 行、CRAP 342）、`server/src/main.rs:383 run_scheduler_cycle`（cc=15、CRAP 240）、`common/src/alert.rs:140 evaluate`（cc=12、嵌套 5）、`server/src/grpc.rs:602 evaluate_alerts`（嵌套 7）。

覆盖率分布（真实数字）：

- 好：`scheduler` 99.7%、`common/alert` 87.3%、`common/auth` 83.2%、`common/node_registry` 79.4%、`common/job` 66.7%、`agent/metrics` 54.0%
- 差：**`storage` 整个 crate 0%**（lib/queries/job_queries/user_queries/alert_queries/audit_queries/aggregation 全 0，8 个文件约 1,490 行）、`server/grpc.rs` 16.9%、`server/handlers.rs` 7.2%、`server/main.rs` 0%、`server/auth_middleware.rs` 0%、`server/ws_handler.rs` 0%、`agent/grpc_client.rs` 0%、`agent/job_executor.rs` 0%、`tui/ui.rs` 4.7%、`tui/api.rs` 0%
- 三个「0% 但没有可执行代码」的文件不算欠账：`common/src/lib.rs`、`protocol/src/lib.rs`（只有 `mod` 声明与 re-export）、`storage/src/migrations.rs`（只有注释的占位模块）
- `protocol/build.rs` 永远 0%：构建脚本不进测试二进制，覆盖率机制覆盖不到

**棘轮基线**：`node .gauntlet/gauntlet.mjs baseline` 记录 **77 项**遗留欠账（46 个超标函数 + 31 个覆盖率不足的文件），存于 `gauntlet-baseline.json`。
开棘轮后 `gate --profile quality` 通过：`RATCHET 基线遗留 77 项；本次容忍（未变差）66 项，比基线变差 0 个函数`。
**规则文件（`gauntlet.config.json`、`gauntlet-baseline.json`、本档案的裁决）是第 0 阶段草稿，需人工确认。**

## 坑

1. **闸门命令只在 node 上跑**。本地 `D:\code\ClusterScope-review\gh-line` 只是与 node 同内容的**只读镜像**，供读/grep 分析；`.gauntlet/gauntlet.mjs` 在本地跑没有意义（commands 是 Linux shell 语法，且没有 `target/`）。流水线文件在本地编辑后用 `scp` 上行，git 操作在 node 上做。
2. **远端无外网**（直连与代理都超时）。因此：cargo 一律加 `--offline`；**不要**跑 `cargo fetch` / `cargo metadata`（会为 Windows/Android 目标解析依赖并因缺包失败，属环境噪声）；装不了任何新工具。
3. **cargo 不在默认 PATH**：每条 `commands` 都以 `sh -c "export PATH=$HOME/.cargo/bin:$PATH; …"` 开头。注意必须是 `sh -c "…"` 这种形式——`doctor` 取命令的第一个 token 去 `which`，直接写 `export …` 会被判成「export NOT FOUND」。
4. **没有 cargo-llvm-cov / cargo-nextest**（装不了）。覆盖率靠 `gauntlet-tools/rust-gate.mjs` 自己实现：`RUSTFLAGS=-Cinstrument-coverage` + `LLVM_PROFILE_FILE` → `llvm-profdata merge` → `llvm-cov export --format=lcov`，JUnit 由 libtest 文本输出转换。系统 LLVM 是 21.1.8（`/usr/bin/llvm-profdata`、`/usr/bin/llvm-cov`），rustc 是 LLVM 22.1.6——**跨版本可读，已验证**。插桩构建用独立的 `CARGO_TARGET_DIR=gauntlet-out/cov-target`，与普通 `target/` 互不污染。
5. **lizard 未装**（pip 无网），`static` 用内置启发式分析器（Rust 按大括号切函数）。`scope` 闸门当前 33/33 通过；若发现函数边界明显错乱，说明内置分析器把 Rust 切错了——应记入结论并请人确认，不要为迁就分析器改代码。
6. **`node .gauntlet/gauntlet.mjs test` 当前退出码 1，原因只有一个**：`features/` 还是空的（`ACCEPTANCE  scenarios=0 … FAIL`），测试本身是 `tests: 44/44 passed` ✅。第 1 阶段写完场景后才会变绿，不要误判成构建/测试坏了。
7. **不要并发跑两条 kit 命令**：`testsGeneric` 每次开始会先删掉 `{out}/junit.xml`、`{out}/lcov.info`，并发跑会互相删报告（我踩过一次）。
8. **server 端到端需要 PostgreSQL 16+，当前环境没有**：`psql`/`postgres`/`initdb` 都不在，`docker` 不在，`podman` 在但**零镜像**且无外网拉镜像。第 5 阶段要验证 server 的 REST/gRPC/DB 行为，需要人提供可达的 PostgreSQL（或批准其它方案）。
9. **PowerShell 单引号**：本地 shell 是 PowerShell，`ssh host '…'` 远程命令必须用单引号包住（否则 `$HOME` 被本地展开）；远程命令里不要用反引号（bash 会当命令替换，我踩过一次）。
10. 远端 128 核 / 502G 内存，构建很快，但**测试本身也很小**（3 秒）——耗时瓶颈只会出现在变异测试和插桩构建上。
11. `features/` 目录由 kit `init` 建好（当前为空，第 1 阶段用）。`gauntlet-out/`、`gauntlet.local.json` 已在 `.gitignore` 里。

## 未纳入闸门测量的东西（不是漏测，是工具读不了）

- `proto/clusterscope.proto`（698 行）、`deploy/**`（8 个文件：2 个 systemd unit、2 个 yaml 模板、docker-compose、Dockerfile.server、install-agent.sh、tui.sh）、`docs/**`、`README.md`、`assets/`
- 原因：`static` 闸门只认 lizard / 内置分析器支持的语言，`.proto`/`.sh`/`.service`/`.yml` 会让 `scope` 闸门**永久失败且无法修复**（离线也装不了能读它们的分析器）。因此 `sources` 只写 `crates/**/*.rs`。
- 代价：这部分内容的正确性**没有自动闸门**，只能靠第 5 阶段 QA 人工核查 + 第 6 阶段报告呈现。
- `survey` 草稿曾建议把 `gauntlet-tools/**/*.mjs` 也纳入 `sources`：**没有采纳**——那是本流水线自己的工具（与 `.gauntlet/**` 同类），不是产品代码；纳入会让闸门去量审查工具本身。
- 上面两条都是规则裁决，**需人工确认**。

## 审查线索（第 1/5/6 阶段可直接取证）

摸底时实际读到/跑到的差异，未做深入判定：

1. **仓库里没有 LICENSE 文件**：README 第 19 行徽章与第 363 行链接指向 `blob/master/LICENSE`（404），`Cargo.toml` 声明 `license = "Apache-2.0"`，但没有 LICENSE/COPYING。
2. **`docs/architecture.md:62` 与实现不符**：文档写保留策略 "2s → 1min → 10min"，代码是原始 24h + 小时级 7 天（`crates/server/src/handlers.rs:303-306`：`RAW_RETENTION_MS = 24h`、`HOURLY_RETENTION_MS = 7d`），README「数据保留」节与代码一致——是 architecture.md 过时。
3. **`clusterscope-server` 不支持 `--help`**：`./target/release/clusterscope-server --help` → `Error: Config file not found: --help`（exit 1）。`crates/server/src/main.rs:182-194` 手工取 `env::args()` 第 1 个参数当配置路径，虽然依赖了 clap 却没用它。`agent`/`tui` 的 `--help` 正常。
4. **WebSocket 还在**：README「已知限制」说 Web 前端已移除，但 `crates/server/src/main.rs:239` 仍注册 `/ws`，`docs/api.md` 也还文档化它（`ws_handler.rs` 135 行，0% 覆盖）。
5. **storage 层零测试**：所有 SQL 查询/聚合/清理（8 个文件约 1,490 行，含多档聚合与保留策略）没有任何测试，也没有集成测试目录；`storage/src/migrations.rs` 是只有注释的占位模块，实际建表在 `DatabasePool::run_migrations` 里内联执行。
6. **测试与文档的口径差**：README 承诺 N 项功能（per-core CPU 条带、Top CPU 进程、进程 USER/COMMAND 降级、告警状态机、三档历史合并等），其中相当一部分落在 0% 覆盖的文件里（`tui/ui.rs`、`server/handlers.rs`、`agent/job_executor.rs`）。
7. **变异测试成本**：内置变异引擎对每个变异体要重编译 + 跑测试（`commands.mutationTest` 已配成不带覆盖率的快路径），本项目热构建 <1s、测试 3s，估算可接受；但 `mutation.scope` 默认 `all`（约 316 个函数），第 4 阶段若启用需评估时长。

## 三线合流素材（本阶段不分析，只记位置）

- **A) 本地 Windows** `D:\code\ClusterScope` —— 基线 `f8ac726` + 未提交的新 Web 前端工作（2026-09-13），只存在一份
- **B) node** `/public/tianyuyang/code/ClusterScope-review/node-line.bundle` —— `19d8fbc` + 12 个未提交文件（2026-08-12），从未推送；另 `local-wip/` 是本地那棵树的原始素材
- **C) 本次审查对象** = 本仓库 `gauntlet/audit-gh-line`，HEAD `f9c080b`（= GitHub `master`，已删除 `web/`，TUI-only）
