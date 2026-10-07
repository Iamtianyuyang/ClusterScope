# QA：无 root 缺陷修复的执行程序（F1–F12）

> 本文件是 `qa/constraints.json` 里本轮追加的 `FIX-*` 约束（13 条）的**执行程序**，供第 5 阶段逐条真跑、第 6 阶段取证。
> 需求原文：「这个项目是要做一个不用 root 的程序」；本轮修的是上一轮审查（分支 `gauntlet/audit-gh-line`，PR #2）在
> 「无 root」这条需求下查出的**四处缺陷**（`NF-01`/`NF-02` + 用户级 unit 缺失 + 无 root 的 PG 路径未文档化）。
>
> 分工：第 2 项（agent 对配置缺失的处理）由 `features/no_root_agent_config.feature` + Rust 验收测试覆盖；
> 本文件对它做**独立复现**（F2–F4，用真二进制、看退出码与输出），并覆盖另外三项**不能靠 Rust 测试覆盖**的修复
> （F1 = 安装脚本；F5–F6 = 用户级/系统级 unit；F7–F9 = 部署文档），外加两条过程约束（F10–F12）。

## 环境与前置（第 5 阶段不需要本文件的作者在场）

- 机器 `node` = `ssh tianyuyang@172.19.133.164`，Linux x86_64，用户 `tianyuyang`、**uid 3000、无 sudo**。
- 仓库：`/public/tianyuyang/code/ClusterScope-review/nr-fixes`，分支 `gauntlet/no-root-fixes`（基点 `7ca587a`）。
- `systemctl --user` **可用**，本机 `Linger=yes`；端口 8080/50051 空闲（2026-10-07 实测）。
- PostgreSQL 16.4（**无 root、源码编译到 HOME**）：连接串 `postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`；
  `psql`/`pg_ctl`/`initdb` 不在 PATH，用 `/public/tianyuyang/code/ClusterScope-review/pg16/bin/` 下的完整路径。
- Rust 1.97.1（`cargo` 不在默认 PATH，脚本里 `export PATH=$HOME/.cargo/bin:$PATH`）；**无外网**，一律 `--offline`。
- 本机**没有 docker / docker-compose**（podman 零镜像）——凡「用 docker 起 PG」的检查只能判 FAIL/N/A（`NR-09` 已记录）。
- 本机有一个**常驻的生产 agent**：`systemctl --user` 的 `clusterscope-agent.service`（`active`、`enabled`，
  2026-09-02 起，2026-10-07 实测 MainPID 266643），二进制 `~/.local/bin/clusterscope-agent`、配置 `~/.config/clusterscope/agent.yaml`。
  **本轮所有检查都不得停下它、不得覆盖它的配置**（这正是 `FIX-01` 要守住的东西）。

## 一键复跑（自包含、离线）

```sh
cd /public/tianyuyang/code/ClusterScope-review/nr-fixes
sh qa/harness/no-root-fixes-checks.sh              # 全量，约 1–2 分钟（含 release 构建 /systemd 起停）
sh qa/harness/no-root-fixes-checks.sh --no-slow    # 跳过 F1 动态探针与 F5 的 systemd 起停（只跑静态部分）
```

每行输出 `<检查号> PASS|FAIL - 说明`，退出码 = FAIL 条数，证据落 `gauntlet-out/qa/evidence/`。
脚本**只按自己记录/自己筛出的 PID 停进程**（`qa/README.md` 硬规矩 1），并且对 `~/.config/systemd/user/`、
`~/.config/clusterscope/` 的每次改动都**先备份、最后还原**。

## 判定口径

| 类别 | 含义 |
|---|---|
| `must-hold` | 本轮的修复目标：应当 PASS；FAIL = 修复没做到 |
| 既有欠账 | 复杂度/CRAP/覆盖率三项 FAIL 是**上一轮的审查结论**，本轮不修、不列入验收（见 `GAUNTLET.md`） |

本轮 13 条约束**全部是 `must-hold`**（`FIX-01`…`FIX-13`）。

### 第 5 阶段的配对要求（重要）

`node .gauntlet/gauntlet.mjs gate --profile full` 的 `constraints` 闸门会把 `qa/constraints.json` 里**每一条**
拿去 `qa/qa-report.json` 找 `"constraint": "<id>"` 的检查：没有配对的算 `unproven`、有一条非 pass 就算 `violated`
（`.gauntlet/lib/checks.mjs` 的 `constraintsGate`）。所以：

- 本轮新增的 13 条 `FIX-*`，每条都要在 `qa/qa-report.json` 里有一条 **status=pass** 的条目；
  一个检查证实多条约束时写数组：`"constraint": ["FIX-02", "FIX-03"]`。
- **既有的 104 条（含 23 条 `NR-*`）也要继续各自配对**——本轮改了 agent 的配置加载语义，
  `NR-06`/`NR-06b`/`NR-12`/`NR-13`/`NR-14` 的断言与话术需要按新行为复跑一遍（见 F3/F5/F10）。
- 证据文件落在 `gauntlet-out/qa/evidence/`（已 `.gitignore`），报告里引用**相对路径**。

## 检查清单

| # | 检查项 | 操作（真实命令，自包含） | 期望结果 | 证实约束 |
|---|---|---|---|---|
| F1 | 安装脚本的停止/替换**不误杀无关 agent** | `sh qa/harness/no-root-fixes-checks.sh` 的 F1 段。核心：① **静态闸门**——脚本里不许有「用字面量名字整机匹配」的 `pkill`/`killall`（判据：该行既没有 `-F`（PID 文件）也**不含 `$`**，即没被变量/路径精确化）；命中就**静态判 FAIL 且不执行**破坏性步骤（共享机器安全阀，见下）；② 否则起一个探针 `<work>/probe/bin/clusterscope-agent -c <work>/probe/agent.yaml`（独立副本，模拟"别人的 agent"），再用 `ssh`/`scp`/`systemctl` **桩**把 `deploy/install-agent.sh` 的"远端"落到 scratch HOME（`HOME=<work>/home-a`）**跑两次**；③ 断言：两次安装 rc=0、探针 `kill -0` 存活、安装前快照的其它 `clusterscope-agent` 进程全部存活、scratch HOME 下只剩 **1** 个 agent、第一次起的那个 PID 已退出（或就是同一个进程 —— 不许默默留着两个）；④ 脚本里要有注释交代停止逻辑 | 探针与既存实例全活；同一次安装只动自己起的进程；脚本里没有字面量名字的整机匹配 | `FIX-01` |
| F2 | 显式 `-c` 指向不存在的文件 → 硬错误、点名文件、**不**回退默认值 | `HOME=<work>/home-b timeout 8 target/release/clusterscope-agent -c <work>/absent/agent.yaml; echo exit=$?` | 退出码 ≠0；输出**逐字**含该路径；含"不存在"措辞（`not found`/`No such file`/`does not exist`）；输出中提到 `.config/clusterscope/agent.yaml`；输出中**没有** `http://localhost:50051` | `FIX-02` `FIX-03` |
| F3 | 不带 `-c` 时默认配置缺失 → 点名默认路径告警后继续跑（不回归 `NR-06`） | `HOME=<work>/home-c timeout 6 target/release/clusterscope-agent; echo exit=$?` | 输出含 `ClusterScope Agent starting`、含 `/etc/clusterscope/agent.yaml`（告警点名默认路径）；exit=124（被 timeout 收掉 = 进程还活着） | `FIX-04` |
| F4 | 启动时创建 node identity 的**父目录** | `mkdir -p <work>/home-d`（刻意不建 `.config`）→ `HOME=<work>/home-d timeout 6 target/release/clusterscope-agent -c <work>/valid.yaml` | `<work>/home-d/.config/node_id` 存在且非空；输出无 `Failed to write node identity`；exit=124 | `FIX-05` |
| F5 | 用户级 unit：名字/路径/真装真启 | `test -f deploy/clusterscope-agent.service -a -f deploy/clusterscope-server.service`；`grep -nE '^(ExecStart\|WantedBy)\|%h' deploy/clusterscope-*.service`；`grep -c 'systemctl --user' README.md`；然后**真装真启**：备份现有 unit → `cp deploy/clusterscope-{agent,server}.service ~/.config/systemd/user/` → `systemctl --user daemon-reload` → `systemctl --user enable --now clusterscope-agent.service` → `systemctl --user is-active/is-enabled` + `systemctl --user status`（落证据）→ 端口空闲时 `enable --now clusterscope-server.service` → `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/api/health` → `disable --now` + 还原 unit 与 `server.yaml` → `daemon-reload` | 两个 unit 文件在仓库里、名字与 README 的 `systemctl --user … clusterscope-{server,agent}` 一致、路径全走 `%h/.local/bin` 与 `%h/.config/clusterscope/`、无 `User=`/`/usr/local/bin`/`multi-user.target`；`daemon-reload`+`enable --now` 均 rc=0；agent unit 装好后**生产实例的 MainPID 不变**（不误伤）；server unit 真起来后 health=200 | `FIX-06` |
| F6 | 系统级 unit 的角色写清楚（可选、需 root 时用） | `grep -nE 'multi-user\.target\|User=clusterscope' deploy/server.service deploy/agent.service`；`grep -nE '需要 root\|需 root\|sudo' README.md`；`head -3 deploy/agent.service deploy/server.service` | 两个系统级 unit 仍在（角色不变），文件头有注释说明是**系统级、需 root**；README 明确写出「系统级是可选项，需要 root 时用」 | `FIX-07` |
| F7 | README 写清**无 root、无 docker** 下怎么准备 PostgreSQL | `grep -nE 'postgres_url' README.md`；`grep -nE 'initdb\|pg_ctl' README.md`；`grep -nE 'pg16\|编译到 HOME\|装到 HOME' README.md`；`grep -n 'docker compose' README.md`；`command -v docker \|\| echo docker-absent` | 两条可行路径都在（① 已有实例直接填 `postgres_url`；② 把 PG 装/编译到 HOME，命令级）；写明**本集群实测走的是**源码编译到 HOME 那条（点名 `pg16` 或「装到 HOME」）；`docker compose` 那条改写成带前提（需要 docker；本集群 `docker-absent`）。grep 只是必要条件——第 5 阶段必须**人工读一遍这一节**，确认两条路径能照抄 | `FIX-08` |
| F8 | README 写清用户级部署的 **linger 前提** | `grep -nE 'loginctl\|Linger' README.md docs/*.md`；`grep -n 'enable-linger' README.md docs/*.md`；`loginctl show-user $USER \| grep '^Linger='` | 文档含 `loginctl show-user` 的检查方法、`Linger=` 的含义（登出后服务会不会被杀）、`enable-linger` 由**管理员/root** 代办的说明；本机 `Linger=yes` 作为实例记录 | `FIX-09` |
| F9 | README 写清**系统级 vs 用户级**两种安装的分工与前提 | `grep -nE 'deploy/clusterscope-(agent\|server)\.service' README.md`；`grep -nE 'deploy/(agent\|server)\.service' README.md`；`grep -n 'systemctl --user' README.md` | 同一节里同时出现「用户级（默认、无需 root）」与「系统级（可选、需 root）」两条路径，各自的前置条件写在旁边 | `FIX-10` |
| F10 | 既有 44 个测试继续全绿，不删/不跳/不弱化 | `sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo test --workspace --offline'`；`git diff 7ca587a -- crates \| grep -E '^-[^-].*#\[(tokio::)?test\]'` | 所有 suite `0 failed`，`passed` 合计 ≥ **44**；diff 里**没有**被删掉的 `#[test]`/`#[tokio::test]` 行 | `FIX-11` |
| F11 | 既有 104 条约束**只追加、不改** | `git diff 7ca587a -- qa/constraints.json \| grep '^-' \| grep -v '^---'`；`node -e 'console.log(JSON.parse(require("fs").readFileSync("qa/constraints.json","utf8")).length)'`；`grep -c '"NR-' qa/constraints.json` | 第一条输出**为空**（没有任何被删/被改的行）；条数 = 104 + 本轮 13 = **117**；`"NR-` 计数不减少（既有 22 条 id 以 `NR-` 开头：`NR-01`…`NR-21` + `NR-06b`，另有 `MRG-02`） | `FIX-12` |
| F12 | 改动范围仅限本轮四项相关文件 | `git diff --name-only 7ca587a`；`git ls-files --others --exclude-standard`；`git diff --name-only 7ca587a -- crates/storage crates/server` | 改动文件全部落在允许集（`crates/agent/src/{config_loader,node_identity,main}.rs`、`crates/agent/tests/**`、`crates/common/src/config.rs`、`deploy/**`、`README.md`、`docs/**`、`features/**`、`qa/**`、`GAUNTLET.md`）；`crates/storage`、`crates/server` **零改动** | `FIX-13` |

## 每条检查的证据落点

| 检查 | 证据文件（`gauntlet-out/qa/evidence/`） |
|---|---|
| F1 | `no-root-fixes-F1-install-agent-stop.txt`（含 `install-1.log` / `install-2.log` 的关键行、PID 快照、断言逐条） |
| F2/F3/F4 | `no-root-fixes-F2-explicit-missing-config.txt` / `-F3-default-missing.txt` / `-F4-identity-parent.txt`（命令 + 完整输出 + 退出码 + 文件清单） |
| F5 | `no-root-fixes-F5-user-units.txt`（unit 文件内容、`systemctl --user status` 全文、health 码、还原过程的 diff） |
| F6–F9 | `no-root-fixes-F6..F9-*.txt`（grep 原文 + 命中的 README 行号） |
| F10–F12 | `no-root-fixes-F10-tests.txt` / `-F11-constraints.txt` / `-F12-scope.txt` |
| 汇总 | `no-root-fixes-checks.txt`（每行 `<ID> PASS\|FAIL - 说明`）+ 脚本退出码 |

## 硬规矩（脚本已遵守，人工复跑时也请遵守）

1. **只按自己记录的 PID 停进程**：这台机器是共享的。`pkill -f clusterscope-agent` 会杀掉
   2026-09-02 起在跑的生产 agent —— 这正是本轮 `FIX-01` 要消灭的行为，检查里**不许**用它。
2. **不要用 `| head` 截断 kit 命令**（SIGPIPE 会让 node 提前死掉，报告是最后才写的；`GAUNTLET.md` 坑 #7）。
3. **不要并发跑两条 kit 命令**（会互相删 `gauntlet-out/junit.xml` / `lcov.info`）。
4. **碰 `~/.config/systemd/user/` 与 `~/.config/clusterscope/` 前先备份**，检查结束必须还原成原样
   （F5 由脚本自动做；手动复跑请照做）。测 `enable --now` 时**不能让生产 agent 的 MainPID 变**。

## 本轮的"反向证据"（规格阶段实跑，用来证明检查真的会咬）

规格阶段（本文件写成时）在**未修复**的代码上跑过一次 `sh qa/harness/no-root-fixes-checks.sh`，
结果见 `qa/README.md`「本轮（no-root 修复）复跑」一节：F1–F6、F10、F12 判 FAIL、F11 判 PASS，
每一条 FAIL 都对应清单里预期的那处缺陷（配置缺失不点名文件、静默回退、身份文件父目录缺失、仓库无用户级 unit、
README 未写 PG/linger 路径……）。**这就是"检查会咬"的证明**；第 2 阶段改完代码后这些必须转 PASS（F11/F12 保持 PASS）。
