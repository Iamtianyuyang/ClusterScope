# QA：无 root 缺陷修复的执行程序（F1–F12）

> 本文件是 `qa/constraints.json` 里本轮追加的 `FIX-*` 约束（14 条）的**执行程序**，供第 5 阶段逐条真跑、第 6 阶段取证。
> 需求原文：「这个项目是要做一个不用 root 的程序」；本轮修的是上一轮审查（分支 `gauntlet/audit-gh-line`，PR #2）在
> 「无 root」这条需求下查出的**四处缺陷**（`NF-01`/`NF-02` + 用户级 unit 缺失 + 无 root 的 PG 路径未文档化）。
>
> 分工：第 2 项（agent 对配置缺失的处理）由 `features/no_root_agent_config.feature` + Rust 验收测试覆盖；
> 本文件对它做**独立复现**（F2–F4，用真二进制、看退出码与输出），并覆盖另外三项**不能靠 Rust 测试覆盖**的修复
> （F1 = 安装脚本；F5–F6 = 用户级/系统级 unit；F7–F9 = 部署文档），外加两条过程约束（F10–F12）
> 与两处流水线工具修复的准入记录（`FIX-14`，复核步骤见「工具修复复核」）。

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

本轮 14 条约束**全部是 `must-hold`**（`FIX-01`…`FIX-14`）。

### 第 5 阶段的配对要求（重要）

`node .gauntlet/gauntlet.mjs gate --profile full` 的 `constraints` 闸门会把 `qa/constraints.json` 里**每一条**
拿去 `qa/qa-report.json` 找 `"constraint": "<id>"` 的检查：没有配对的算 `unproven`、有一条非 pass 就算 `violated`
（`.gauntlet/lib/checks.mjs` 的 `constraintsGate`）。所以：

- 本轮新增的 14 条 `FIX-*`，每条都要在 `qa/qa-report.json` 里有一条 **status=pass** 的条目；
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
| F5 | 用户级 unit：名字/路径/真装真启 | `test -f deploy/clusterscope-agent.service -a -f deploy/clusterscope-server.service`；`grep -nE '^(ExecStart\|WantedBy)\|%h' deploy/clusterscope-*.service`；`grep -c 'systemctl --user' README.md`；然后**真装真启**（F5 段**自带** `server.yaml`：由 `deploy/server.yaml.example` 生成、连 `127.0.0.1:5432`；机器上那份 `~/.config/clusterscope/server.yaml` 指向 5433，不能复用——见「工具修复复核」②）：备份现有 unit → `cp deploy/clusterscope-{agent,server}.service ~/.config/systemd/user/` → `systemctl --user daemon-reload` → `systemctl --user enable --now clusterscope-agent.service` → `systemctl --user is-active/is-enabled` + `systemctl --user status`（落证据）→ 端口空闲时 `enable --now clusterscope-server.service` → `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/api/health` → `disable --now` + 还原 unit 与 `server.yaml` → `daemon-reload` | 两个 unit 文件在仓库里、名字与 README 的 `systemctl --user … clusterscope-{server,agent}` 一致、路径全走 `%h/.local/bin` 与 `%h/.config/clusterscope/`、无 `User=`/`/usr/local/bin`/`multi-user.target`；`daemon-reload`+`enable --now` 均 rc=0；agent unit 装好后**生产实例的 MainPID 不变**（不误伤）；server unit 真起来后 health=200 | `FIX-06` |
| F6 | 系统级 unit 的角色写清楚（可选、需 root 时用） | `grep -nE 'multi-user\.target\|User=clusterscope' deploy/server.service deploy/agent.service`；`grep -nE '需要 root\|需 root\|sudo' README.md`；`head -3 deploy/agent.service deploy/server.service` | 两个系统级 unit 仍在（角色不变），文件头有注释说明是**系统级、需 root**；README 明确写出「系统级是可选项，需要 root 时用」 | `FIX-07` |
| F7 | README 写清**无 root、无 docker** 下怎么准备 PostgreSQL | `grep -nE 'postgres_url' README.md`；`grep -nE 'initdb\|pg_ctl' README.md`；`grep -nE 'pg16\|编译到 HOME\|装到 HOME' README.md`；`grep -n 'docker compose' README.md`；`command -v docker \|\| echo docker-absent` | 两条可行路径都在（① 已有实例直接填 `postgres_url`；② 把 PG 装/编译到 HOME，命令级）；写明**本集群实测走的是**源码编译到 HOME 那条（点名 `pg16` 或「装到 HOME」）；`docker compose` 那条改写成带前提（需要 docker；本集群 `docker-absent`）。grep 只是必要条件——第 5 阶段必须**人工读一遍这一节**，确认两条路径能照抄 | `FIX-08` |
| F8 | README 写清用户级部署的 **linger 前提** | `grep -nE 'loginctl\|Linger' README.md docs/*.md`；`grep -n 'enable-linger' README.md docs/*.md`；`loginctl show-user $USER \| grep '^Linger='` | 文档含 `loginctl show-user` 的检查方法、`Linger=` 的含义（登出后服务会不会被杀）、`enable-linger` 由**管理员/root** 代办的说明；本机 `Linger=yes` 作为实例记录 | `FIX-09` |
| F9 | README 写清**系统级 vs 用户级**两种安装的分工与前提 | `grep -nE 'deploy/clusterscope-(agent\|server)\.service' README.md`；`grep -nE 'deploy/(agent\|server)\.service' README.md`；`grep -n 'systemctl --user' README.md` | 同一节里同时出现「用户级（默认、无需 root）」与「系统级（可选、需 root）」两条路径，各自的前置条件写在旁边 | `FIX-10` |
| F10 | 既有 44 个测试继续全绿，不删/不跳/不弱化 | `sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo test --workspace --offline'`；`git diff 7ca587a -- crates \| grep -E '^-[^-].*#\[(tokio::)?test\]'` | 所有 suite `0 failed`，`passed` 合计 ≥ **44**；diff 里**没有**被删掉的 `#[test]`/`#[tokio::test]` 行 | `FIX-11` |
| F11 | 既有 104 条约束**只追加、不改** | `git diff 7ca587a -- qa/constraints.json \| grep '^-' \| grep -v '^---'`；`node -e 'console.log(JSON.parse(require("fs").readFileSync("qa/constraints.json","utf8")).length)'`；`grep -c '"NR-' qa/constraints.json` | 第一条输出**为空**（没有任何被删/被改的行——`FIX-13` 的修订以**追加** `FIX-14` 落地，没有改写原文）；条数 = 104 + 本轮 14 = **118**；`"NR-` 计数不减少（既有 22 条 id 以 `NR-` 开头：`NR-01`…`NR-21` + `NR-06b`，另有 `MRG-02`） | `FIX-12` |
| F12 | 改动范围仅限本轮四项相关文件 | `git diff --name-only 7ca587a`；`git ls-files --others --exclude-standard`；`git diff --name-only 7ca587a -- crates/storage crates/server` | 改动文件全部落在允许集（`crates/agent/src/{config_loader,node_identity,main}.rs`、`crates/agent/tests/**`、`crates/common/src/config.rs`、`deploy/**`、`README.md`、`docs/**`、`features/**`、`qa/**`、`GAUNTLET.md`，以及 `FIX-14` 修订加入的 `gauntlet-tools/**`）；`crates/storage`、`crates/server` **零改动**；脚本里的负例自检（`crates/storage`/`crates/server`/`Cargo.toml`/`gauntlet.config.json`/`.gauntlet/**` 必须仍判越界）5/5 通过；另见「工具修复复核」⑤（2026-10-07 允许集加入 `demo/*`，并补 1 条正例自检） | `FIX-13` `FIX-14` |

## 每条检查的证据落点

| 检查 | 证据文件（`gauntlet-out/qa/evidence/`） |
|---|---|
| F1 | `no-root-fixes-F1-install-agent-stop.txt`（含 `install-1.log` / `install-2.log` 的关键行、PID 快照、断言逐条） |
| F2/F3/F4 | `no-root-fixes-F2-explicit-missing-config.txt` / `-F3-default-missing.txt` / `-F4-identity-parent.txt`（命令 + 完整输出 + 退出码 + 文件清单） |
| F5 | `no-root-fixes-F5-user-units.txt`（unit 文件内容、`systemctl --user status` 全文、health 码、还原过程的 diff） |
| F6–F9 | `no-root-fixes-F6..F9-*.txt`（grep 原文 + 命中的 README 行号） |
| F10–F12 | `no-root-fixes-F10-tests.txt` / `-F11-constraints.txt` / `-F12-scope.txt` |
| 汇总 | `no-root-fixes-checks.txt`（每行 `<ID> PASS\|FAIL - 说明`）+ 脚本退出码 |

## 工具修复复核（第 5 阶段必做，对应 `FIX-14`）

本轮编码阶段在声明范围之外动过**两个流水线工具文件**（Leader 逐行核验两个 diff 后裁决「准予纳入」；来源、diff 要点与
「为什么以追加 `FIX-14` 落地而不改写 `FIX-13` 原文」见 `qa/constraints.json#FIX-14`）。它们不是产品代码，但都会改变
「怎么读结果」，所以准入条件不是「相信它」，而是**在 QA 里独立复核**。下面四项复核的结论要落成 `qa/qa-report.json` 里
`"constraint": "FIX-14"` 的 **status=pass** 条目，证据放 `gauntlet-out/qa/evidence/`。

**① rust-gate 排除的确实是产品二进制，而不是测试**

```sh
cd /public/tianyuyang/code/ClusterScope-review/nr-fixes
export PATH=$HOME/.cargo/bin:$PATH
cargo test --workspace --offline --no-run --message-format=json > /tmp/artifacts.jsonl 2>/dev/null
node -e 'const fs=require("fs");
const a=fs.readFileSync("/tmp/artifacts.jsonl","utf8").split("\n").map((l)=>{try{return JSON.parse(l)}catch{return null}})
  .filter((m)=>m&&m.reason==="compiler-artifact"&&m.executable);
const show=(isTest)=>a.filter((m)=>(m.profile.test===true)===isTest).map((m)=>m.target.name+" ("+m.target.kind.join("/")+")").join(", ");
console.log("profile.test=false（产品二进制，rust-gate 不 spawn）:", show(false));
console.log("profile.test=true （测试二进制，rust-gate 照跑）:", show(true));'
```

`cargo test` 会为集成测试引用的 `CARGO_BIN_EXE_*` 额外构建**产品二进制**（`profile.test === false`）。期望（2026-10-07
返工实测）：`profile.test=false` 恰好 **1 个**：`clusterscope-agent (bin)`；`profile.test=true` **8 个**：`common (lib)`、
`scheduler (lib)`、`protocol (lib)`、`storage (lib)`、`no_root_agent_config (test)`、`clusterscope-agent (bin)`、
`clusterscope-server (bin)`、`clusterscope-tui (bin)`。注意 `clusterscope-agent` **同名出现两次**（产品 + 它自己的测试壳），
只有 `profile.test` 分得开——只看名字会把常驻守护进程当测试跑，那正是当初 `gate --profile coder` 挂死 31 分钟的原因。
静态对照：`gauntlet-tools/rust-gate.mjs:98/104/119`（`isTest: m.profile?.test === true` 只对 `true` 的产物 spawn，
其余仍进覆盖率对象表）。

**② 测试总数与 `cargo test` 输出一致**

```sh
node gauntlet-tools/rust-gate.mjs --out /tmp/nrfix-rg > /tmp/nrfix-rg.log 2>&1
grep -oE 'tests="[0-9]+"' /tmp/nrfix-rg/junit.xml | head -1
sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo test --workspace --offline' 2>&1 | grep -oE '[0-9]+ passed' | awk '{s+=$1} END{print s}'
```

期望两边**相等**（2026-10-07 返工实测都是 **59**；`/tmp/nrfix-rg.log` 的 suite 明细：`clusterscope-agent (bin): 11/11`、
`clusterscope-server (bin): 6/6`、`clusterscope-tui (bin): 3/3`、`common (lib): 21/21`、`no_root_agent_config (test): 6/6`
（= 6 个验收场景）、`scheduler (lib): 12/12`）。`junit.xml` 的根元素 `<testsuites tests="59">` 是汇总，下面 12 个
`<testsuite>` 加起来也是 59，**别把根元素算两遍**。同时核对「没有跳过测试」：rust-gate 日志第二行应当是
`8 test binaries (+1 product binary/binaries, built for CARGO_BIN_EXE_*, not run)`——被排除的就是 ① 里那 1 个产品二进制。

**③ F5 的断言集合未被削减**

```sh
git show b6149ee^:qa/harness/no-root-fixes-checks.sh > /tmp/f5-before.sh
awk '/^f5\(\)/,/^}/' /tmp/f5-before.sh | grep -o 'A "[^"]*"' | sort > /tmp/f5b.txt
awk '/^f5\(\)/,/^}/' qa/harness/no-root-fixes-checks.sh | grep -o 'A "[^"]*"' | sort > /tmp/f5a.txt
diff -u /tmp/f5b.txt /tmp/f5a.txt && echo "F5 断言集合一致"
```

期望**零差异**（2026-10-07 返工实测：各 30 条、逐行相同）。F5 现在**自带** `server.yaml`（由 `deploy/server.yaml.example`
生成、连 `127.0.0.1:5432` + 本检查的 `jwt_secret`）；机器上那份 `~/.config/clusterscope/server.yaml` 的 `postgres_url`
指向 `127.0.0.1:5433`（那里没有实例在听），复用它会卡在连库超时里，使 health 断言在**任何正确实现**上都不可能通过。
备份/还原逻辑与断言集合都未动。

**④ 允许集本身没有被放宽（F12 段的负例自检 + F11 口径未动）**

`sh qa/harness/no-root-fixes-checks.sh` 的 F12 段对 5 条越界路径做负例自检（`crates/storage/src/lib.rs`、
`crates/server/src/lib.rs`、`Cargo.toml`、`gauntlet.config.json`、`.gauntlet/gauntlet.mjs`），断言它们**全部**仍被判越界；
证据见 `gauntlet-out/qa/evidence/no-root-fixes-F12-scope.txt` 的「负例自检」段。另外 F11 段的口径**没有改动**
（仍是「`git diff 7ca587a -- qa/constraints.json` 零删行」）：`FIX-13` 的修订以**追加** `FIX-14` 的形式落地，
`FIX-13` 原文与既有 104 条逐字节未动。

**⑤ 允许集加入 `demo/*`（2026-10-07 Leader 裁决，本阶段落笔）**

`demo/*.json` 是 **QA 阶段的法定产物**（`gauntlet-qa` 技能：可回放的演示脚本），第 5 阶段写演示文件属预期行为——
第 5 阶段报告的 F12 失败即由此而来（`qa/README.md`「一条要请 Leader 裁决的范围口径缺口」）。
落地方式：只在 `in_scope()` 的允许集里加 `demo/*` 这一项（该行上方注释写明理由与裁决日期），**不动** `qa/constraints.json`
（`FIX-13` 原文与既有 104 条逐字节未动，`F11` 的「零删行」口径原样有效），`FIX-01`…`FIX-14` 的判据无一处改变。
强度未削弱：5 条负例自检原样 **5/5**，另**新增 1 条正例自检**（`demo/*` 必须判**允许**，守住这次修订）。
复跑证据：`qa/evidence/no-root-fixes-FIX15-scope-amendment.txt` 状态 C、
`qa/evidence/no-root-fixes-FIX15-F12-scope-after-amendment.txt`。

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
