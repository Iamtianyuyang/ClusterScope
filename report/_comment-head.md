# ClusterScope 审查 · 证据包小结（第 6 阶段 · 含 no-root 增补）

> **单文件证据包（先看这个）**：`report/review.html`
> **合流方案**：`report/merge-plan.md`（M1–M10） · **使用教程**：`docs/review-howto.md`
> **审查对象**：GitHub 已发布线 `f9c080b`（TUI-only，40 个提交） · **报告提交**：`gauntlet/audit-gh-line` 的 `[report]` 提交（`8fb31d1` = 上一版 + no-root 两个增补）

## 5 分钟审阅路线（入口）

1. **先看 4 处**（`report/review.html` 内）：① 第 1 节前 3 条 findings（F-01 / F-16 / F-02，都是"功能实际不可用"级别）；
   ② **第 2 节「无 root 合规」**（一句话结论 + 4 条核心结论 + NF-01/NF-02 两条 major + `NRM1`–`NRM8` 现状表）；
   ③ 第 3 节闸门面板（✅ 与 ❌ 分开呈现 + `verdict: pass` 的语义）；④ 第 6 节 N1–N14（审查自身的 14 处不精确）。
2. **再点 4 个演示**（`gauntlet-out/evidence/demos/`）：`04-rest-security-audit-logs.html`、`07-history-daily-tier-missing.html`、`02-quality-gates-fail.html`、**`09-no-root-system-vs-user-units.html`**（同批还有 `10-no-root-linger-and-config-fallback.html`）。
3. **最后看两处结构证据**：架构图 `gauntlet-out/evidence/diagrams/clusterscope-runtime.html`、合流方案 `report/merge-plan.md`（**M5 需产品决策**、**M10 = no-root 不变量 `NRM1`–`NRM8`**）。

## 结论（三句话）

- **证据包本身合格**：**104 条约束**（81 条原样 + 23 条 no-root 增补）逐条真跑、**18 条 finding** 有原始证据、**10 个演示**可回放、架构图通过 Archify 校验。
- **产品不健康**：硬阈值下 complexity 21/316（maxCC 23）、CRAP 45/316（maxCRAP 552）、覆盖率 20.7% 三闸门 ❌；
  外加两个"功能不可用"缺陷（审计端点恒 500、90 天天级历史永不返回）与四个安全边界缺口（无登录限速、token 不可吊销、审计只覆盖 2 个动作、任务参数无上限）。
- **"不用 root"这条需求：运行时成立，交付面不成立**——uid 3000 / `CapEff=0` 下 server + agent + TUI 全功能可用（实测），
  但随仓库发的是**系统级** unit（非 root 装不上）、仓库里 **0 个**用户级 server unit、无 root 的数据库路径 README **一处未写**；
  另加两条 major：**NF-01**（干净 HOME + 缺 `-c` 文件 → exit 1，报错从不提配置文件）与 **NF-02**（`deploy/install-agent.sh:80` 的 `pkill -f clusterscope-agent` 会杀掉同用户所有 agent）。

## 18 条 findings 速览

| id | severity | 一句话 | B 线是否已修 |
|---|---|---|---|
| F-01 | major | `GET /api/audit-logs` 恒 500（`SELECT *` 与 `username` 列不匹配） | ✅ `eac070e` |
| F-16 | major | 审计 COUNT 语句零绑定（被 500 掩盖的第二个缺陷） | ✅ 同一个 `eac070e` |
| F-02 | major | 天级（90 天）历史永不返回、错误被静默吞 | ⚠️ 另一条修法，需比对 |
| **NF-01** | **major** | **agent 在「干净 HOME + `-c` 缺失文件」下 exit 1，报错不提配置文件；配置缺失时静默回退** | ❌ 三棵树都没有修法（需新写） |
| **NF-02** | **major** | **`deploy/install-agent.sh:80` 的 `pkill -f clusterscope-agent` 会杀掉同用户所有 agent** | ❌ 三棵树都没有修法（1 行改动） |
| F-04 | major | 硬阈值下三个质量闸门 FAIL（审查结论，不修） | — |
| F-06 | major | 15 个配置键写了不生效（清单只记 11 个） | ✅ 大部分 |
| F-07 | major | read-only 鉴权边界与两份文档都不一致 | ⚠️ 部分 |
| F-08 | major | 登录无 IP/全局限速：可无限枚举用户名 | ✅ B-wip |
| F-09 | major | access token 不可吊销（无 jti/黑名单） | ⚠️ 部分（refresh 批量吊销已有） |
| F-10 | major | 审计只覆盖 create_job / stop_job | ✅ B-wip 13 处 |
| F-11 | major | 任务参数只有两条校验 | ✅ B-wip `MAX_ARGS` |
| F-13 | major | 没有任何 CI（`.github/` 不存在） | ❌ |
| F-03 | minor | `jobs.pid` 从不落库 | ⚠️ 部分 |
| F-05 | minor | README 的 `force → SIGKILL` 不存在 | ✅ B-wip |
| F-12 | minor | 8 条文档不符 + 默认口令 `admin` | ⚠️ 部分 |
| F-14 | minor | 没有自动架构检查（arch 跳过） | ❌ |
| F-15 | minor | `retry_count`/`max_retries` 是死列 | ✅ B 提交 |

**合流前必修**：F-01 + F-16 + F-02 + F-07 ~ F-11 + F-12 里的默认口令 + **NF-01 + NF-02**（no-root，Leader 已裁决，改动都很小）。

## no-root 一节摘要（2026-10-07 增补：23 条 `NR-*` + M10 不变量）

- **成立（实测）**：运行时零特权——uid **3000**、**`CapEff=0000000000000000`**、零 HOME 外写入；
  server（8080/50051 监听、`/api/health`=200）、agent、TUI（pty 正常渲染）全功能可用；`/proc` 权限不足时按 README 承诺降级；
  端口都 >1024，不需要 `CAP_NET_BIND_SERVICE`；零配置文件也能起 server（env-only 路径 `lsof` 命中 0 处系统路径）。
- **不成立**：① 部署件与承诺自相矛盾——随仓库发的 `deploy/{server,agent}.service` 是**系统级** unit
  （`User=clusterscope`、`/usr/local/bin`、`/etc/clusterscope`、`/var/lib/clusterscope`、`/var/log/…`、`WantedBy=multi-user.target`），
  非 root 实测装不上（`systemctl link` → `Interactive authentication required`；`cp` → `Permission denied`），而 README 通篇写 `systemctl --user`；
  ② 仓库 **0 个**用户级 server unit——README:288-289 的 `systemctl --user restart clusterscope-server` 在**干净机器**上必然失败
  （本机那份 `~/.config/systemd/user/clusterscope-server.service` 是 2026-08-10 的**手写私货**，不属于仓库）；
  ③ 无 root 的数据库路径未文档化——README:57/87 承诺「无 root 时可用 `docker compose up`」，本机无 docker、podman 零镜像且无外网，
  而真正可行的路径（源码编译 PG 16.4 到 HOME）README **0 处**记载。
- **两条新 finding（都升级为合流前必修）**：**NF-01**、**NF-02**（见上表；NF-02 第 5 阶段**未执行**——跑一下就会误杀常驻 agent，取证来自源码 + 实时进程表）。
- **linger 双向记录（原样写明）**：本机当前 `Linger=yes`（`loginctl show-user tianyuyang`），用户级服务**活过登出（本机实测 ✅）**；
  但本机这个 `yes` **很可能是 Leader 前期探测时执行 `loginctl enable-linger` 造成的，不是集群默认值**（探测初期读到 `no`）。
  反向：`linger=no` 的节点上，最后一个会话结束时 logind 会停掉该用户的 per-user manager → **用户级 agent 会随登出而死**，
  而开启 linger 通常需要管理员/root → 该分支在本机**无法复现**（无 root、不许改集群配置），只作语义记录，**不替集群做假设**。
- **诚实项**：`qa/merge-plan-requirements.md` 里 `NRM5` 的 `grep -E "…\|…"` 在 GNU grep 下**匹配空集**（ERE 里 `\|` 是字面竖线），
  它的「0 命中」是**模式假象**；正确模式命中 1 处无害的 `libc::setsid()`（`job_executor.rs:128-129`，进程组设置，非提权）→ 作为 discovery `N14` 原样并入。
  另：作者 harness 的 `NR6 PASS` 是**假阳性**（判据取了 `grep` 的退出码，问候行在崩溃前就打印了），`NR-06` 的最终判定以第 5 阶段为准。
- **合流必须回答**：**M10 的 `NRM1`–`NRM8`**（逐条命令与输出见 `report/merge-plan.md` 的 M10）。
  现状：`NRM1`/`NRM3`/`NRM5` 成立、`NRM2` 部分不成立（= NF-01）、`NRM4` 待合流时逐文件验、`NRM6`/`NRM7` 不成立、`NRM8` 前置就绪。
  **合流后待办**：补用户级 unit（`NRM6`）、文档化无 root 的数据库路径（`NRM7`）。

## 合流一句话

以 **C（GitHub `f9c080b`）为基线**：先把 B 的 `eac070e` cherry-pick 进来（实测只有 4 个文件 / 6 个冲突块 / 约 140 行），
再按文件 graft B 的 12 个未提交改动（1784+/740−，不在任何提交或 bundle 里，**先打包校验**），
再做 no-root 的两处必修（`NF-01`：缺配置明确报错 + 建父目录；`NF-02`：去掉 `pkill -f`，改按 PID/精确匹配），
A 的独有资产（web 27 文件、`common/{dedup,metrics,sequence}.rs`、`tests/integration_test.rs`、`deploy/nginx.conf`）按 M5 的产品决策处理。
**不要整体 `git merge 19d8fbc`**（实测 35 个文件冲突，含 `modify/delete`）。
合流后还要把 **M6 步骤 7 的两个待办**（补用户级 unit、文档化无 root 的 DB 路径）做掉，并按 **M10** 的三条命令复验。

---

# 以下是 kit 自动生成的仪表盘与逐条明细（原始记录，未经修饰）
