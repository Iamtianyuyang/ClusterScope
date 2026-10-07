# 修复「不用 root」这条需求下的四处缺陷（栈式 PR，目标分支 `gauntlet/audit-gh-line`）

> 需求原文：**这个项目是要做一个不用 root 的程序**
> 本 PR 只含**修复 + 配套规格/证据**；审查产物（`report/review.html`、`qa/merge-plan.md`、`docs/review-howto.md` 等）在**父 PR #2**（`gauntlet/audit-gh-line`）里。
> 分支：`gauntlet/no-root-fixes` @ `1279b48`，基于 `gauntlet/audit-gh-line` 的 tip `7ca587a`。

## 这次修了什么

| # | 缺陷（审查结论） | 修复 | 关键文件 |
|---|---|---|---|
| 1 | **NF-02**：`deploy/install-agent.sh` 的 nohup 回退分支先跑 `pkill -f clusterscope-agent`，会杀掉**同用户下所有** agent（含本机常驻实例 266643） | 只停自己启动的那个进程：自己的 PID 文件 + `/proc/<pid>/cmdline` 核对；PID 复用/不匹配时打印 `pidfile points at an unrelated process … leaving it alone` | `deploy/install-agent.sh` |
| 2 | **NF-01**：显式 `-c <不存在的文件>` 被**静默**忽略，agent 带着 `http://localhost:50051` 默认值继续跑，报错从不提配置文件 | 显式 `-c` 缺失 = **非 0 退出 + 逐字点名该路径 + 点名 `$HOME/.config/clusterscope/agent.yaml`**；默认 `/etc/clusterscope/agent.yaml` 缺失 = 点名告警后继续（不破坏 NR-06「不崩」） | `crates/agent/src/config_loader.rs` |
| 3 | 仓库发出去的 systemd unit 全是**系统级**（`User=`、`/usr/local/bin`、`multi-user.target`），非 root 装不上；没有用户级 server unit | 新增 `deploy/clusterscope-agent.service` / `deploy/clusterscope-server.service`（`%h`、`WantedBy=default.target`、不含 `User=`），uid 3000 下**真装真启**；系统级 unit 保留并在文件头/README 注明「可选 / 需 root」 | `deploy/*.service` |
| 4 | README 的数据库路径只有 `sudo` + docker：无 root、无 docker 的用户照做**跑不起来**；linger 前提没写 | 补两条实测过的无 root PG 路径（含源码编译到 `~/pg16`）、`loginctl show-user`/`enable-linger` 前提、用户级 vs 系统级分工表 | `README.md` |
| 5 | 干净 HOME 下写 node identity 失败（父目录不存在） | 写之前 `create_dir_all` 父目录 | `crates/agent/src/node_identity.rs` |

**另含两处流水线工具修复**（都是工具自身缺陷，不碰产品行为，Leader 逐行核验后准入，记录在 `qa/constraints.json` 的 `FIX-14`）：

- `gauntlet-tools/rust-gate.mjs`：`cargo test` 会为 `env!("CARGO_BIN_EXE_*")` 构建**产品二进制**，旧代码把它们也当测试目标启动——agent 是常驻守护进程，闸门就此挂死（实测 31 分钟）。现在只启动 `profile.test === true` 的产物；**59 个测试照跑、断言零弱化**。
- `qa/harness/no-root-fixes-checks.sh` 的 F5：改为**自带** server 配置，不再复用机器上指向死端口（5433）的 `~/.config/clusterscope/server.yaml`；`/api/health=200` 这条断言从此可判；断言集合 30 条**零差异**。

## 怎么验的

| 证据 | 结果 |
|---|---|
| `node .gauntlet/gauntlet.mjs gate --profile coder` | **PASS** — spec 6 场景 · build ✅ · tests **59/59** · acceptance **6/6** |
| `sh qa/harness/no-root-fixes-checks.sh` | **PASS=12 FAIL=0**（F1–F12，含 F5 的 systemd 真装真启与逐字节还原断言） |
| 约束闸门（kit 自己的 `constraintsGate`） | 118 条约束 **113 met**；本轮 14 条 `FIX-*` **14/14 met**；非 met 的 5 条均为上一轮已记录的环境不可验证/不适用项（OPS-05 docker、OPS-09 纯 CPU、OPS-10 防火墙、FE-01 前端 N/A、MRG-01 属父 PR） |
| 演示（真实回放） | `demo/11-no-root-fixes-install-agent-safety.json`（探针存活、只动自己的 PID）、`demo/12-no-root-fixes-user-units-and-config-errors.json`（server unit 真起 `/api/health=200`、显式 `-c` 缺失硬错误） |
| 共享机器纪律 | 全程只按 PID 停自己起的进程；常驻 agent **PID 266643 未变**、`is-active=active`；F5 触碰的 4 个文件 `cmp` **逐字节一致**、`Linger=yes` |

**未重算的重型闸门（如实标注，非美化）**：`complexity` / `crap` / `coverage` 三闸门仍是上一轮审查记录的 ❌（335 函数 21 超标、maxCRAP 552、行覆盖 22.9%）——本轮只做「无 root」修复，不修这三项、也不重算；`mutation` 本轮未完成（full profile 跑到 `[22/736]` 被人工停止，日志原样留证）。

## 残余与已知局限

- **F-17**：同名常驻 unit 已在跑时，agent 侧「真装真启」在这台机器上只是 no-op（`MainPID` 不变既是 FIX-06 要的「不打扰」，也让该断言 trivially 成立）；真正被观测到的冷启是 server unit。
- **F-18**：审计里「静默回退」的表述已被本轮修复取代，属历史记录；默认配置路径缺失时「告警 + 继续跑」这条分支仍然成立。
- **F-22**：F5 会临时把操作者的 `~/.config/systemd/user/clusterscope-agent.service` 换成仓库版，必须逐字节还原；第 5 阶段曾出现一次「看起来一样」被保留，已还原。复跑前请自行留副本（见 `report/no-root-fixes.html` 第 5.3 节）。
- **F-20**：F12 的范围口径曾因 `demo/*` 新交付物判越界，已由 Leader 裁决修订（`1279b48`），负例自检仍 5/5。

## 复跑

```sh
cd /public/tianyuyang/code/ClusterScope-review/nr-fixes
export PATH=$HOME/.cargo/bin:$PATH
node .gauntlet/gauntlet.mjs gate --profile coder        # 11 秒，期望 GATE coder: PASS
sh qa/harness/no-root-fixes-checks.sh                   # 1–2 分钟，期望 PASS=12 FAIL=0（跑前先备份那 4 个文件）
```

证据包（单文件、离线可看）：**`report/no-root-fixes.html`**；逐条原始输出：`qa/evidence/`。
