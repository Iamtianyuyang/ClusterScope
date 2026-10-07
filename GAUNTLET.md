# GAUNTLET.md — 项目档案

commit: `669f995`（第 1 阶段补充见文末「审查清单（第 1 阶段产出）」；**2026-10-07 增补见文末「第 1 阶段增补：no-root 维度」**）　base: `master`　更新：2026-10-07　适配器：commands　棘轮：**关**（硬阈值判定；基线 77 项保留但惰性）

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
| 全量质量闸门 | `node .gauntlet/gauntlet.mjs gate --profile quality` | 7s（热） | **硬阈值下 FAIL**（complexity / crap / coverage），见「硬阈值下的现状」 |
| 运行交付物（冒烟） | `./target/release/clusterscope-agent --help`、`clusterscope-tui --help` | — | 两者 exit 0 并打印帮助 |
| 运行 server | `./target/release/clusterscope-server <config.yaml>` | — | 需要 PostgreSQL 16+（**已就绪**，见下）；**不支持 `--help`**（见「审查线索」） |
| PostgreSQL 是否在跑 | `/public/tianyuyang/code/ClusterScope-review/pg16/bin/pg_ctl -D /public/tianyuyang/code/ClusterScope-review/pgdata -l /tmp/pg-server.log status` | <1s | exit 0 = 在跑；启动把 `status` 换成 `start`（**未验证**，当前已在跑） |
| PostgreSQL 连通性自检 | `/public/tianyuyang/code/ClusterScope-review/pg16/bin/psql "postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope" -tAc "select version();"` | <1s | 返回 `PostgreSQL 16.4 …` |

### PostgreSQL 16.4（第 5 阶段 QA 的前置，已就绪）

- 位置：`/public/tianyuyang/code/ClusterScope-review/pg16`（从源码编译，**无 root、自包含**），数据目录 `/public/tianyuyang/code/ClusterScope-review/pgdata`
- 监听 `127.0.0.1:5432`，unix socket 在 `/tmp`；连接串
  `postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`
- 实测（2026-10-07）：`pg_ctl … status` → `pg_ctl: server is running (PID: 4176115)`，进程命令行
  `postgres "-D" ".../pgdata" "-p" "5432" "-k" "/tmp" "-c" "listen_addresses=127.0.0.1"`；
  `psql … -tAc "select version();"` → `PostgreSQL 16.4 on x86_64-pc-linux-gnu, compiled by gcc (GCC) 11.5.0 20240719, 64-bit`
- 日志：`/tmp/pg-server.log`。**`psql` / `initdb` / `pg_ctl` 都不在 PATH**，用完整路径。
- 这不是 `deploy/docker-compose.yml` 那套：本机/远端仍然**没有 docker 镜像**（podman 在但零镜像、无外网），
  compose 那条路走不通，QA 一律用这个手装实例。server 的配置模板见 `deploy/server.yaml.example`。

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

> 本节是**指标原值**（与棘轮开关无关）。判 PASS/FAIL 的口径在 2026-10-07 由用户裁决改为**硬阈值**，
> 失败清单、距离与函数级明细见下面的「硬阈值下的现状（用户裁决，2026-10-07）」。

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
   另外**不要用 `| head` 截断 kit 命令的输出**：`head` 提前关掉管道会让 node 收到 SIGPIPE 直接死掉，
   而所有报告（`gate.json` / `loop-*.json` / `next.md`）都是**最后才写**的——你会拿到一份陈旧报告还以为它跑完了（我踩过一次）。
   要截断就 `> /tmp/x.log 2>&1` 再 `tail`/`grep` 那个文件。
8. **server 端到端需要 PostgreSQL 16+——2026-10-07 已就绪**：`/public/tianyuyang/code/ClusterScope-review/pg16`（源码编译，无 root，自包含）跑在 `127.0.0.1:5432`，连接串 `postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`；探测/启动命令见「构建、测试、运行」。仍然**没有** `docker`，`podman` 零镜像且无外网，所以 `deploy/docker-compose.yml` 那条路走不通；`psql`/`initdb`/`pg_ctl` 也不在 PATH。
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

## 硬阈值下的现状（用户裁决，2026-10-07）

> **2026-10-07 用户裁决：关闭棘轮、硬阈值判定——需人工确认。**
> 本节是审查报告的核心证据：硬阈值下**真实失败清单 + 距离**，具体到函数级。

**规则变更（本次唯一的规则改动，逐条列出）**

| 文件 | 改动 | 说明 |
|---|---|---|
| `gauntlet.config.json` | `"ratchet": { "enabled": true }` → `"enabled": false` | 用户明确指令；`git diff` 只有这 1 行 |
| `gauntlet-baseline.json` | **未删、未改** | 77 项欠账仍是既有事实，现在只是惰性文件；删除等于销毁证据 |
| `sources` / `exclude` / `thresholds` / 架构规则 / 产品代码 / 测试 | **一律未动** | — |

副作用（别误读）：`gate` 输出里不再有 `RATCHET` 行；`gauntlet-out/ratchet.json` 停在 03:15 的旧值
（`{"baseline":77,"legacy":66,"worse":0}`），是**惰性残留，不要当本轮证据用**。

**测量口径**：远端 `gh-line`，分支 `gauntlet/audit-gh-line`，revision `113a365` + 上面那 1 行配置改动；
命令 `node .gauntlet/gauntlet.mjs gate --profile quality`（热构建 6.9s）。下列每个数字都取自 `gauntlet-out/*.json`
（`gate.json` / `static.json` / `crap.json` / `coverage.lines.json` / `duplication.json` / `tidy.json` / `next.md` / `loop-quality.json`）。
**复现性**：提交后在干净树 `669f995` 上原样复跑，闸门结论与距离**逐项一致**（`GATE quality: FAIL` exit 1；
`next` → CONTINUE exit 1，97 项 / 409.505，`loop-quality.json` 记 `commit: "669f995"`）。

### 各闸门结论与退出码（硬阈值，不做修饰）

| 闸门 | 结论 | 数字 |
|---|---|---|
| build | ✅ PASS | `cargo build --workspace --all-targets --offline` exit 0 |
| tests | ✅ PASS | **44/44 通过**，0 失败，0 忽略 |
| scope | ✅ PASS | 33/33 文件被内置分析器解析，`scope=100.0%`，`failedUnits=0`，`engine=builtin`（lizard 未装） |
| complexity | ❌ **FAIL** | 316 个函数中 **21 个超标**（合计 **28 条**阈值违规）；maxCC=23、maxLines=196、maxNesting=7、maxParams=9 |
| warnings | ✅ PASS | clippy **0** 条（`cargo clippy … -- -D warnings` 也 exit 0） |
| tidy | ✅ PASS | `cargo fmt --all --check` exit 0，0 发现 |
| duplication | ✅ PASS | **0.0%**：7030 代码行中 0 处克隆（min 100 tokens，跨目录 0） |
| crap | ❌ **FAIL** | 316 个函数中 **45 个超标**，maxCRAP=**552** |
| arch | ➖ 跳过 | 未配置 `commands.arch`（离线装不了 cargo-modules/cargo-deny） |
| coverage | ❌ **FAIL** | 行覆盖 **20.7%**（1381/6686），阈值 90% |
| **GATE quality** | ❌ **FAIL** | `GATE quality: FAIL` → **exit code 1** |

另：`node .gauntlet/gauntlet.mjs test` 在硬阈值下**仍然 exit 1**，唯一原因是 `features/` 为空
（`ACCEPTANCE scenarios=0 passed=0 failed=0 missing=0` → `ACCEPTANCE gate: FAIL`），测试本身 44/44 通过。
第 1 阶段写完场景后才会变绿——**不要**误判成构建/测试坏了。

### 距离（kit 0.3.0 的机械口径，`next --profile quality --reset` → CONTINUE，exit 1）

```
结论：CONTINUE　剩余 97 项，离阈值的距离 409.505　失败的闸门：complexity, crap, coverage
```

距离 = 待修项数 + 每项超出阈值的**相对比例**之和（`kit/lib/next.mjs:120-122`）。按 kit 的公式逐项复算，
与 kit 记录的 409.505 **完全一致**（`Math.round(…*1000)/1000`）：

| 闸门 | 项数 | 超出部分 | 距离贡献 |
|---|---|---|---|
| complexity | 21 | 16.002 | 37.002 |
| crap | 45 | 271.009 | 316.009 |
| coverage | 31 | 25.494 | 56.494 |
| **合计** | **97** | **312.505** | **409.505** |

**读法**：crap 一项占距离的 77%，其中 `ui.rs:599 node_panel`（CRAP 552 / 阈值 8）与 `ui.rs:1024 draw_process`
（342/8）两个函数贡献最大；coverage 一侧 21 个文件是 0%（每项权重拉满 1.0）。距离要归零 = 97 项全清。

### 1. 超标函数（complexity 闸门）：21 个函数 / 28 条违规

阈值：圈复杂度 >10、函数长度 >60 行、嵌套 >4 层、参数 >7 个。

按**违规类型**拆开（同一函数可跨类）：

**圈复杂度（5 项）**

| 函数 | cc | 超 |
|---|---|---|
| `crates/tui/src/ui.rs:599` `node_panel` | 23 | 2.30x（+13） |
| `crates/tui/src/ui.rs:1024` `draw_process` | 18 | 1.80x（+8） |
| `crates/server/src/main.rs:383` `run_scheduler_cycle` | 15 | 1.50x（+5） |
| `crates/common/src/alert.rs:140` `evaluate` | 12 | 1.20x（+2） |
| `crates/server/src/main.rs:314` `run_background_tasks` | 12 | 1.20x（+2） |

**函数长度（18 项）**

| 函数 | 行数 | 超 |
|---|---|---|
| `crates/tui/src/ui.rs:1024` `draw_process` | 196 | 3.27x（+136） |
| `crates/tui/src/ui.rs:599` `node_panel` | 170 | 2.83x（+110） |
| `crates/common/src/alert.rs:140` `evaluate` | 135 | 2.25x（+75） |
| `crates/tui/src/ui.rs:781` `draw_trend_full` | 114 | 1.90x（+54） |
| `crates/server/src/main.rs:383` `run_scheduler_cycle` | 110 | 1.83x（+50） |
| `crates/server/src/grpc.rs:141` `report_metrics` | 110 | 1.83x（+50） |
| `crates/tui/src/ui.rs:1223` `draw_cpu_processes` | 109 | 1.82x（+49） |
| `crates/tui/src/ui.rs:1432` `draw_alerts` | 96 | 1.60x（+36） |
| `crates/server/src/main.rs:234` `build_http_router` | 79 | 1.32x（+19） |
| `crates/server/src/handlers.rs:217` `get_metrics_history` | 75 | 1.25x（+15） |
| `crates/server/src/handlers.rs:420` `create_job` | 71 | 1.18x（+11） |
| `crates/server/src/handlers.rs:631` `create_alert_rule` | 69 | 1.15x（+9） |
| `crates/server/src/grpc.rs:451` `update_job_status` | 69 | 1.15x（+9） |
| `crates/server/src/main.rs:314` `run_background_tasks` | 66 | 1.10x（+6） |
| `crates/server/src/handlers.rs:31` `login` | 65 | 1.08x（+5） |
| `crates/server/src/grpc.rs:252` `submit_job` | 64 | 1.07x（+4） |
| `crates/server/src/grpc.rs:602` `evaluate_alerts` | 63 | 1.05x（+3） |
| `crates/tui/src/ui.rs:463` `draw_topbar` | 62 | 1.03x（+2） |

**嵌套深度（4 项）**

| 函数 | 嵌套 | 超 |
|---|---|---|
| `crates/server/src/grpc.rs:602` `evaluate_alerts` | 7 | 1.75x（+3） |
| `crates/server/src/ws_handler.rs:154` `handle` | 6 | 1.50x（+2） |
| `crates/server/src/grpc.rs:320` `get_pending_jobs` | 6 | 1.50x（+2） |
| `crates/common/src/alert.rs:140` `evaluate` | 5 | 1.25x（+1） |

**参数个数（1 项）**：`crates/storage/src/audit_queries.rs:8` `insert_audit_log` — 9 个 > 7（+2）。

**按文件分布**：`tui/ui.rs` 6 个、`server/grpc.rs` 5 个、`server/handlers.rs` 4 个、`server/main.rs` 3 个、
`server/ws_handler.rs` 1 个、`common/alert.rs` 1 个、`storage/audit_queries.rs` 1 个。
**没有**任何函数超参数以外的 storage 函数；`scheduler`、`protocol` 完全干净。

### 2. CRAP：45 个函数超标，最大值 552（阈值 8）

`maxCRAP=552`；分布：

| 区间 | 函数数 |
|---|---|
| > 100 | 4 |
| 50–100 | 9 |
| 20–50 | 6 |
| 8–20 | 26 |
| ≤ 8（合格） | 271 |

**按 crate**：`server` 21/86 超标（最大 240）、`tui` 19/61（最大 **552**）、`agent` 4/32（最大 90）、
`common` 1/79（最大 12.07）、`storage` 0/31、`scheduler` 0/27。

**Top 15（`static`+`crap` 交集，全部 `cov=0%` 除最后一行）**

| 函数 | CRAP | cc | 覆盖率 |
|---|---|---|---|
| `crates/tui/src/ui.rs:599` `node_panel` | **552** | 23 | 0% |
| `crates/tui/src/ui.rs:1024` `draw_process` | 342 | 18 | 0% |
| `crates/server/src/main.rs:383` `run_scheduler_cycle` | 240 | 15 | 0% |
| `crates/server/src/main.rs:314` `run_background_tasks` | 156 | 12 | 0% |
| `crates/tui/src/ui.rs:1223` `draw_cpu_processes` | 90 | 9 | 0% |
| `crates/server/src/grpc.rs:602` `evaluate_alerts` | 90 | 9 | 0% |
| `crates/agent/src/config_loader.rs:5` `load_config` | 90 | 9 | 0% |
| `crates/server/src/main.rs:181` `load_config` | 72 | 8 | 0% |
| `crates/server/src/auth_middleware.rs:37` `readonly_middleware` | 72 | 8 | 0% |
| `crates/tui/src/ui.rs:781` `draw_trend_full` | 56 | 7 | 0% |
| `crates/server/src/grpc.rs:141` `report_metrics` | 56 | 7 | 0% |
| `crates/server/src/handlers.rs:217` `get_metrics_history` | 56 | 7 | 0% |
| `crates/tui/src/ui.rs:463` `draw_topbar` | 56 | 7 | 0% |
| `crates/server/src/handlers.rs:631` `create_alert_rule` | 42 | 6 | 0% |
| `crates/server/src/ws_handler.rs:69` `broadcast` | 42 | 6 | 0% |
| `crates/common/src/alert.rs:140` `evaluate` | 12.07 | 12 | 92.2% |

**关键读数**：45 项里 **44 项的覆盖率是 0%**，唯一例外是 `alert::evaluate`（92.2% 覆盖、纯粹因复杂度 12 超标）。
也就是说 CRAP 这次**不是复杂度的锅，是"没测试"的锅**——先把覆盖率拉起来，CRAP 会大面积自然消解
（CRAP = cc²·(1-cov)³ + cc，cov→1 时退化为 cc）。

### 3. 覆盖率缺口：总体 20.7%（1381/6686），阈值 90%

要到 90% 还需再覆盖 **4636 行**（90% 门槛 = 6017 行）。**31 个文件低于阈值**，其中 **21 个是 0%**。

| crate | 覆盖率 | 行 |
|---|---|---|
| `storage` | **0.0%** | 0/1353 |
| `tui` | 4.0% | 51/1269 |
| `server` | 7.5% | 133/1773 |
| `agent` | 26.5% | 271/1021 |
| `common` | 76.9% | 635/826 |
| `scheduler` | 99.7% | 291/292 |
| `protocol` | 0%（仅 build.rs/lib.rs 声明） | — |

**0% 的文件（21 个）**

- `storage`（7 个，1296 行）：`queries.rs` 274、`job_queries.rs` 265、`lib.rs` 248、`alert_queries.rs` 230、
  `user_queries.rs` 158、`aggregation.rs` 97、`audit_queries.rs` 81 —— **整个 crate 零测试**
- `server`（4 个，893 行）：`main.rs` 331、`ws_handler.rs` 135、`auth_middleware.rs` 92、另有 `handlers.rs` 7.2%
- `agent`（5 个，519 行）：`job_executor.rs` 208、`grpc_client.rs` 170、`main.rs` 99、`config_loader.rs` 27、`node_identity.rs` 15
- `tui`（2 个）：`api.rs` 104、`main.rs` 86
- 声明类：`common/src/lib.rs` 6、`protocol/build.rs` 14、`protocol/src/lib.rs` 10、`storage/src/models.rs` 122

**非零但远低于阈值的关键文件**：`server/handlers.rs` 7.2%（54/747）、`server/grpc.rs` 16.9%（79/468）、
`tui/ui.rs` 4.7%（51/1079）、`common/config.rs` 36.2%（17/47）、`agent/metrics.rs` 54.0%（271/502）。

**需人工确认**：`common/src/lib.rs`、`protocol/src/lib.rs`、`protocol/build.rs`、`storage/src/models.rs`
这 4 个文件按闸门口径各算 1 项 0%（共 152 行）——上一轮档案认为前两个只有 `mod` 声明与 re-export、
`storage/src/migrations.rs` 只有注释。哪些真属"无可执行语句"、是否该移出测量范围，**只能由人裁决**
（我不能自己改 `sources` / `exclude`）。`protocol/build.rs` 是构建脚本，覆盖率机制天然覆盖不到。

### 4. 其它闸门读数（都是硬阈值下的实测）

- **重复代码**：0 处克隆 / 7030 代码行 = **0.0%**（阈值 3%，min 100 tokens，跨目录 0）→ PASS
- **clippy 告警**：**0** 条；`cargo clippy --workspace --all-targets --offline --message-format=short` exit 0，
  再叠 `-- -D warnings` 仍 exit 0 → warnings 闸门 PASS
- **rustfmt**：`cargo fmt --all --check` **exit 0，0 发现** → tidy 闸门 PASS
  （注意：项目没有 `rustfmt.toml` / `clippy.toml`，量的是**工具默认规则**）
- **静态测量范围**：`scope=100.0%`，33/33 文件、33 个翻译单元、`failedUnits=0`、316 个函数、7030 代码行；
  分析器是**内置启发式**（`engine=builtin`，lizard 未装），Rust 按大括号切函数
- **arch**：未配置 `commands.arch`，闸门跳过——**证据包里会列为"需人工确认"**

### 5. 对后续阶段的影响（务必先读）

- 本次审查模式是**只派 0 → 1 → 5 → 6，不改产品代码**，所以 complexity / crap / coverage 这三个 FAIL
  **不会被修**——它们是**审查结论**，不是待办清单。第 6 阶段的报告必须原样呈现 `GATE quality: FAIL`，
  不能写成 PASS。
- 硬阈值下**任何包含 quality 闸门的 profile 都会 FAIL**。第 1/5/6 阶段若用 `next --profile quality`
  判收尾，它永远不会返回 DONE（会一直 CONTINUE）；各阶段请用自己 profile 的闸门，并把这三项失败
  预先判定为"本审查不修、记录在案"，否则会误触发返工。
- 第 5 阶段（QA）现在**有 PostgreSQL 16.4 可用**（见「构建、测试、运行」），server 端到端可以做。

## 审查清单（第 1 阶段产出，2026-10-07）

> 审查模式：**只跑 0 → 1 → 5 → 6**，不派编码/清理/加固阶段，**不改产品代码、不改测试、不改文档**。
> 因此 `ACCEPTANCE`（场景 ↔ 验收测试）与 `quality`（complexity/crap/coverage）两个闸门在本审查里都是 **N/A**：
> 前者不写 `features/*.feature`（用户裁决，见下），后者三项 FAIL 是审查结论而非待办。

**入口**（第 5 阶段从这里开始读，`qa/README.md` 有「一键复跑」）：

| 文件 | 内容 | 规模 |
|---|---|---|
| `qa/README.md` | 审查清单总览、环境、一键复跑命令、三条硬规矩 | — |
| `qa/constraints.json` | 可机器检查的约束（id / 断言 / 依据行号 / 检查命令 / 期望 / 判定） | **104 条**：81 条原样（46 `must-hold`、29 `finding`、4 `na`、2 `long`）+ **23 条本次增补**（`NR-01`…`NR-21`、`NR-06b`、`MRG-02`） |
| `qa/no-root.qa.md` | **NR1–NR21 + MRG-02**：无 root 维度的执行程序（第 5 阶段逐条真跑） | 21+1 条 |
| `qa/harness/no-root-checks.sh` | 上表的自包含执行脚本（`sh qa/harness/no-root-checks.sh [--no-slow]`），证据落 `gauntlet-out/qa/evidence/` | 1 个脚本 |
| `qa/build-gates.qa.md` | G1–G12 真实闸门复现 | 12 条 |
| `qa/docs-consistency.qa.md` | D1–D22 文档 ↔ 实现 | 22 条 |
| `qa/security.qa.md` | S1–S17 认证/鉴权/注入面/审计 | 17 条 |
| `qa/concurrency.qa.md` | C1–C16 任务生命周期/调度/去重/保留/WS/迁移 | 16 条 |
| `qa/deploy-ops.qa.md` | O1–O21 `deploy/` ↔ 代码、端口、TUI 冒烟、N/A 说明 | 21 条 |
| `qa/merge-plan-requirements.md` | M1–**M10** 三棵树合流**必答问题**（第 6 阶段的输入；M10 = no-root 不变量 NRM1–NRM8） | 10 题 |
| `qa/harness/*.sh` `*.mjs` `*.py` `*.sql` | **已实测跑通**的执行脚本与夹具 | 9 个脚本 + 1 SQL |

**第 1 阶段已实测确认的缺陷**（第 5 阶段只需复跑取证，不要重新发现）：

1. `GET /api/audit-logs` **恒 500**：`audit_queries.rs:70-79` 用 `SELECT *`，模型字段 `user`（`models.rs:142`）与表列 `username` 不匹配。
2. **天级（90 天）历史永不返回**：同一 SQL 过滤条件在 psql 里能取到 4 行（`qa/harness/diag-daily.sql`），REST 返回 0 行——`handlers.rs:274-285` 的 `if let Ok(...)` 把 `DATE` 列解码失败吞掉了。
3. `clusterscope-server --help` → `Config file not found: --help`（exit 1），clap 依赖未用（`main.rs:182-194`）。
4. `jobs.pid` **从不落库**（`grpc.rs:497-506` 恒传 `None`），agent 日志里有真实 pid；`retry_count/max_retries` 是死列（无重试）。
5. 11 个**死配置键**（写了不生效）：`log_level` / `disk_mounts` / `collect_process_details`（agent）+ `redis_url` / `prometheus_enabled` / `prometheus_addr` / `ws_*`×4 / `tls_enabled`（server）。
6. 登录**无 IP 限速**（20 次错误登录对不存在的用户全 401，无 429）；access token **不可吊销**；审计只覆盖 create_job / stop_job 两个动作。
7. `README:233` 的「read-only 时 GET 全开放」不完全成立（`/api/users` 这类 admin 级 GET 仍 401）；`README:358` 说 `active_alerts` 无数据为 null，实际恒为 0。
8. 无 LICENSE 文件（README:19/363 链接 404）；`docs/architecture.md:62` 保留策略过时；`README:355` 的「force → SIGKILL」不存在。

**规格闸门的裁决（需人工确认）**：`node .gauntlet/gauntlet.mjs gate --profile specifier` → **FAIL**，唯一原因是
`spec: 0 feature(s), 0 scenario(s)`（kit 的判据是 `scenarios > 0`，`.gauntlet/lib/adapter-commands.mjs:73-78`）。
用户已裁决「本阶段不写 `features/*.feature`」（不改产品代码 ⇒ 无法落地 Rust 验收测试 ⇒ ACCEPTANCE 不适用），
所以这条闸门在本审查里记 **N/A**，全部可验证内容落在 `qa/` 下。**没有**为了让闸门变绿而写占位场景（那属于「改弱断言让闸门通过」）。
若需要 spec 闸门变绿，唯一办法是写 ≥1 个 feature/scenario —— 需用户改口，不能由 agent 自行决定。

**第 5 阶段注意**：`qa/harness/*.sh` 只按 PID 文件停进程（这台机器共享，禁止 `pkill -f clusterscope`）；
证据落在 `gauntlet-out/qa/evidence/`；每条检查写进 `qa/qa-report.json` 时用 `"constraint": "<约束 id>"` 与约束配对。

## 第 1 阶段增补：no-root 维度（2026-10-07）

> **增补原因**：用户需求原文「**这个项目是要做一个不用 root 的程序**」。上一个阶段（同一第 1 阶段的审查清单）
> 把「无 root」只当成一条**文档不符**记录（`DOC-21`：README 用 `systemctl --user`、unit 却是系统级），
> **没有把它当成一等验收维度**——既没有「普通用户下能不能真跑」的可判定条目，也没有「合流后不得引入 root 依赖」的不变量。
> 本次增补把这条需求拆成可复现、可判真假的约束与执行程序。**审查模式不变**（只跑 0→1→5→6，不改产品代码/测试/`deploy/`/README/docs）。

**新增条数**：`qa/constraints.json` 追加 **23 条**（`NR-01`…`NR-21`、`NR-06b`、`MRG-02`），原 **81 条一字未改**（只追加；
`gauntlet.config.json` 未动）。合计 **104 条**。`qa/merge-plan-requirements.md` 新增 **M10**（含 `NRM1`–`NRM8` 展开）。

| 维度 | 新增约束 | 结论（2026-10-07 实测，uid 3000，无 sudo） |
|---|---|---|
| 运行时全功能 | `NR-01` `NR-02` `NR-03` `NR-06` `NR-06b` `NR-18` `NR-20` `NR-21` | **成立**：server 起在 8080/50051（health=200）、agent 起得来、TUI 在 pty 渲染、NVML/sysfs 可读；端口 >1024 无需特权；零配置文件也行（env-only 路径 lsof 命中 0 处系统路径） |
| 路径默认值 | `NR-03` `NR-04` `NR-06` `NR-06b` `NR-19` | 默认值全部落在 HOME（`dirs` 6.0.0/XDG：`~/.config/node_id`、`~/.local/state/clusterscope-agent`）；`/etc`、`/var/lib`、`/var/log`、`/usr/local/bin` 对本用户**一律不可写**；**HOME 只读时 agent 硬失败**（`Failed to create log directory`，SSH/HPC 共享节点上要当心） |
| 安装/部署件 | `NR-05` `NR-13` `NR-14` `NR-15` `NR-16` | **两套并存且矛盾**：`deploy/*.service` 是系统级（`User=clusterscope`、`/usr/local/bin`、`/var/lib/clusterscope`、`multi-user.target`、还 `After=redis.service`）→ 非 root **装不上**；`install-agent.sh` 是用户级（`~/.local/bin`、`~/.config/clusterscope`、`systemd --user`/`nohup`）→ **可用且已在生产运行**（本机 2026-09-02 起 user unit active）。README:288-289 的 server 管理命令**仓库里没有对应 unit**（本机那份 `~/.config/systemd/user/clusterscope-server.service` 是 2026-08-10 **手写**的，非仓库产物） |
| 持久化 | `NR-11` `NR-12` | 本机 `Linger=yes`（与上次交接的「未验证」不同）：user 服务断开 SSH 仍活；但 linger 是**每机**配置（要 root 才能 enable-linger），且 `nohup` 回退**没有** `Restart=always` → 文档「agent 常驻/60s 自动重注册」只在 systemd --user+linger 路径下成立 |
| 外部依赖 | `NR-09` `NR-07` | README:87 的 `docker compose up` 在本集群**不成立**（无 docker/docker-compose、无外网）；可行路径「源码编译 PG 到 HOME」**未文档化**；另有隐性前置：裸启动 server 会因 `jwt_secret` 守卫**拒绝启动** |
| 文档对账 | `NR-10` | README 11 处「无 root/systemctl --user」逐条判：`:15/:21/:56/:89/:211/:290/:291/:293/:321` **成立**；`:87`、`:288-289` **不成立**（后者的命令只在这台已手配过 unit 的机器上有效） |
| 合流不变量 | `MRG-02`（=`M10`） | 合流不得引入新的 root 依赖；系统级 unit 必须修掉或明确区分；判据 `NRM1`–`NRM8` + 合流后重跑 `sh qa/harness/no-root-checks.sh` |

**复跑入口（第 5 阶段）**：`cd /public/tianyuyang/code/ClusterScope-review/gh-line && sh qa/harness/no-root-checks.sh`
（自包含，只按自己的 PID 停进程；`--no-slow` 跳过两条带 sleep 的检查；证据落 `gauntlet-out/qa/evidence/`）。
本次实跑：**PASS=23 FAIL=0，退出码 0**（2026-10-07，uid 3000，node `lyy-node03`）。
脚本会在自己的块里创建并**删除**一个专用 user unit（`nr-probe-unit.service` / `nr-persist-unit.service`），
**不动**机器上既有的 `clusterscope-agent.service`（那个是 2026-09-02 起一直在跑的老实例，`NR16` 把它当证据而不是靶子）。

**本次增补中的四条重测提醒（与交接材料不同之处）**：

1. `Linger` **是 `yes` 不是 `no`**（`loginctl show-user tianyuyang` → `Linger=yes`，且本机自 2026-09-02 的 user 级 agent 一直在跑）——
   「登出后服务存活」这条在本机成立；但别处可能是 no，故写成 `NR-11` 逐机复跑的检查，而不是一次性结论。
2. **本机有一个手写的用户级 server unit**：`~/.config/systemd/user/clusterscope-server.service`
   （`ExecStart=$HOME/.local/bin/clusterscope-server $HOME/.config/clusterscope/server.yaml`、`WantedBy=default.target`，
   2026-08-10 建立、当前 disabled/inactive）。它证明「用户级跑 server」真的可行，也说明 README:288-289 的命令
   **只在这台已手配过的机器上有效**——仓库里没有任何脚本生成它（`NR-13`），合流时这是 `NRM6` 要补的东西。
3. **`agent -c /etc/clusterscope/agent.yaml` 在文件缺失时不会报错**（`config_loader.rs:9-11` 只看 `exists()`），
   会静默改用 `~/.config` 默认值——照抄系统级 unit 的人会不知情地用错配置（`NR-06`）。与之相对，
   `deploy/server.service` 里 server 的**位置参数**路径缺失时 server 会直接 `Config file not found` 退出（`DOC-03`）。
4. **探针要 bind 8080/50051**：刚停掉的 server 会让端口短暂处于 `TIME_WAIT`，脚本用 `SO_REUSEADDR` + 重试 3 次；
   `ss` 显示已被别人占用时，该端口记为「busy 未探测」而不是 FAIL（这台机器是共享的）。


