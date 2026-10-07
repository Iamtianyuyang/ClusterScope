#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Append the MRG6-* constraints (M6 merge round) to qa/constraints.json.

纯追加：把新条目插在数组最后的 `  }` 与 `]` 之间（`  }` 行与 `]` 行逐字节不动），
所以 `git diff --numstat <base> -- qa/constraints.json` 的第二列必须是 0 —— 这是
no-root-fixes-checks.sh 的 F11「只追加」判据。
"""
import io
import json
import os
import sys

REPO = sys.argv[1] if len(sys.argv) > 1 else "."
PATH = os.path.join(REPO, "qa", "constraints.json")

E = []              # appended in order


def add(cid, text, basis, verify, expect, verdict, qa, severity):
    E.append({
        "id": cid,
        "text": text,
        "basis": basis,
        "verify": verify,
        "expect": expect,
        "verdict": verdict,
        "qa": qa,
        "severity": severity,
    })


add(
    "MRG6-01",
    "合流后的构建与测试门槛成立：`cargo build --workspace --all-targets --offline` 退出码 0，`cargo test --workspace --offline` 退出码 0、0 失败、通过数**不少于 59**（合流前实测 59 passed / 0 failed）；全程离线，**不得引入需要联网下载的新依赖**。",
    "本轮任务书硬性约束 6（远端无外网、一律 --offline、不得引入新依赖）；report/merge-plan.md 的 M9「构建 / 单元测试」两行；qa/merge-m6.qa.md §1（合流前基线：59/0）与 §5.1",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && export PATH=$HOME/.cargo/bin:$PATH && cargo build --workspace --all-targets --offline && cargo test --workspace --offline；或 sh qa/harness/merge-m6-checks.sh --static（M6-01/M6-02 两段，含 passed/failed 计数）",
    "构建 exit 0；测试 0 失败、passed ≥ 59；证据 gauntlet-out/qa/evidence/merge-m6-{build,tests}.txt",
    "must-hold",
    "merge-m6.qa.md#M6-01",
    "blocker",
)

add(
    "MRG6-02",
    "M6 步骤 1（cherry-pick `eac070e`）之后，审计端点的两个缺陷必须一次修掉：`GET /api/audit-logs` 从恒 500 变成 200（F-01），且审计查询的 COUNT 语句**绑定了它自己的参数**（F-16）——判据是「按任意筛选条件查询都返回条数与 total 一致」，不是「SQL 文本长得像」。",
    "report/merge-plan.md 的 M6 步骤 1 与 M8「合流前必修」（F-01 + F-16 同一个提交）；qa/qa-report.json 的 F-01/F-16 条目；qa/merge-m6.qa.md §2 表 1.5",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh --static 之后运行 sh qa/harness/server-up.sh false && sh qa/harness/api-checks.sh; sh qa/harness/server-down.sh（见 merge-m6-checks.sh 的 M6-11）；另由 features/merge_m6_audit_queries.feature 的 4 条场景的同名测试在库上验证",
    "`CHECK DOC-GET-AUDIT-LOGS: PASS expected=200 actual=200`；api-checks 的 PASS 数从 18 升到 19；M6-11 PASS",
    "must-hold",
    "merge-m6.qa.md#M6-11",
    "blocker",
)

add(
    "MRG6-03",
    "本轮 22 条新验收场景（features/merge_m6_audit_queries.feature 4 条、merge_m6_auth_hardening.feature 8 条、merge_m6_job_safety.feature 6 条、merge_m6_legacy_assets.feature 4 条）各自要有一个**名字逐字包含场景名**的、**通过的**测试；加上既有的 6 条（no_root_agent_config.feature）共 28 条，验收闸门不得出现「没有对应测试」或失败项。",
    "本轮任务书「场景名必须逐字出现在通过的 Rust 测试名里（小写 + 折叠空白的子串匹配，用 snake_case）」；gauntlet-adapter-commands 的验收测试命名契约；qa/merge-m6.qa.md §1 第 3 行",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && node .gauntlet/gauntlet.mjs test（看 ACCEPTANCE 一栏）；node .gauntlet/gauntlet.mjs gate --profile coder",
    "ACCEPTANCE 一栏 28 个场景全部匹配到通过的测试；`GATE coder` 的 acceptance 项 ✅",
    "must-hold",
    "merge-m6.qa.md#M6-02",
    "blocker",
)

add(
    "MRG6-04",
    "M6 步骤 2 必须**一次一个文件**地 graft B 的 12 个未提交文件（素材只许用冻结副本 `../backup-b-wip/b-wip.tar.gz`），每个文件之后都构建 + 测试 + 提交；出现「B 的改法 vs C 的改法」冲突时取语义更全的一侧并写下理由，**不得整体 `git merge 19d8fbc`**（实测 35 个文件冲突且含 modify/delete），也不得把 C 线独有的修复（去重 key、status 映射、orphan 回收、marker 重放、`remove_rule` 级联、NVML/per-core metrics、`$n` 占位符）静默丢掉。",
    "本轮任务书硬性约束 1 与表格步骤 2；report/merge-plan.md 的 M4（35 个冲突文件）与 M6 步骤 2 的推荐顺序；qa/merge-m6.qa.md §3",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && for f in $(cat ../backup-b-wip/list.txt); do printf '%s ' \"$f\"; diff -u \"$f\" gauntlet-out/m6/b-wip/$f | grep -c '^[+-]'; done；逐文件残留差异落 gauntlet-out/qa/evidence/merge-m6-graft-<file>.diff；git log --oneline 8601ac9..HEAD 里每个 graft 文件一次提交",
    "12 个文件逐个提交、每个提交点构建与测试都是绿的；没有 B 侧功能被静默丢弃（残留差异只有 C 侧 API 适配）",
    "must-hold",
    "merge-m6.qa.md#2",
    "blocker",
)

add(
    "MRG6-05",
    "合流**不得引入新的 root 依赖**（NRM3 + NRM4）：① 12 个 graft 文件里不允许出现 `/etc/`、`/var/lib`、`/var/log`、`/usr/local`、`pre_exec` 提权、`chown`、`CAP_*` 之类的新命中（命中必须逐条解释或改回）；② `crates/**` + `deploy/install-agent.sh` + `deploy/tui.sh` 的系统路径默认值实质命中**不超过合流前的 3 处**（`server/main.rs:186` 的 argv 默认值、`agent/config_loader.rs:11` 的 `DEFAULT_CONFIG_PATH`、`common/config.rs:38` 的配置键默认值——三处都可被 CLI/配置覆盖），同一 grep 的全部命中不超过 7 行（另 4 行是注释与测试夹具）。",
    "本轮任务书硬性约束 2（合流不得引入新的 root 依赖）与 qa/merge-plan-requirements.md 的 M10 / NRM3+NRM4；qa/merge-m6.qa.md §3 表 2.3 与 §1 第 10 行",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh --static（M6-05 段）；逐文件的 NRM4 命令：git diff 8601ac9 -- <file> | grep -nE '/etc/|/var/lib|/var/log|/usr/local|pre_exec|setuid|setgid|chown|CAP_'",
    "M6-05 PASS：实质命中 ≤ 3、全部命中 ≤ 7；逐文件 diff 无未解释命中",
    "must-hold",
    "merge-m6.qa.md#M6-05",
    "blocker",
)

add(
    "MRG6-06",
    "合流**不得新增特权原语**（NRM5）：`crates/**` 里 `setuid|setgid|pkexec|chown|sudo ` 的命中必须仍为 **0**；`pre_exec` + `setsid` 那一处（给子进程建进程组，不是提权）不超过合流前的 2 行，且 `pre_exec` 旁边必须仍是 `libc::setsid()`；两个 `Cargo.toml` 的新依赖要逐个写明用途，不得引入要求系统级权限的路径。",
    "本轮任务书硬性约束 2；qa/merge-plan-requirements.md 的 NRM5（并纠正了 `grep -E '...\\|...'` 的模式假象）；qa/merge-m6.qa.md §1 第 11 行与 §5.2",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh --static（M6-06 段）；git diff 8601ac9 --stat -- Cargo.toml crates/*/Cargo.toml",
    "提权族 0 行；进程组族 ≤ 2 行且 pre_exec 旁有 setsid；新依赖逐条有用途说明",
    "must-hold",
    "merge-m6.qa.md#M6-06",
    "blocker",
)

add(
    "MRG6-07",
    "M6 步骤 3 必须把 A 的独有资产接回主线并且**只接这三样**（M5 裁决 = 先 TUI-only）：`crates/common/src/metrics.rs`（含 `MetricsAggregation`）、`crates/storage/src/conversions.rs`（按 C 的 API 改签名）、`tests/integration_test.rs`（按 C 的 API 改签名后放入 `tests/`）；每一项之后都要构建 + 测试 + 提交。`web/` 与 `deploy/nginx.conf` 本轮**不做**。",
    "本轮任务书表格步骤 3 与硬性约束（M5 = 先 TUI-only，web 走独立分支）；report/merge-plan.md 的 M3/M5/M6 步骤 3；qa/merge-m6.qa.md §4",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && test -f crates/common/src/metrics.rs && grep -q 'pub mod metrics;' crates/common/src/lib.rs && test -f crates/storage/src/conversions.rs && grep -q 'pub mod conversions;' crates/storage/src/lib.rs && test -f tests/integration_test.rs && cargo test --workspace --offline",
    "三样资产都在主线里、构建与测试全绿；`web/` 与 `deploy/nginx.conf` 零改动（M6-04 的冻结面）",
    "must-hold",
    "merge-m6.qa.md#3",
    "major",
)

add(
    "MRG6-08",
    "**不得删、跳过、注释掉或弱化任何既有测试与验收场景**：`cargo test --workspace --offline` 的通过数相对合流前（59）只增不减、0 失败；`git diff 8601ac9 -- crates tests | grep '^-.*#\\[test\\]'` 必须为空；`features/*.feature` 里既有场景的含义不得改动（本轮只允许**新增** 4 个 feature 文件）；`gauntlet.config.json`（sources/exclude/thresholds/ratchet）与 `.gauntlet/` 不得改动。",
    "本轮任务书硬性约束 3（不得删/跳过/弱化既有测试、不得改 features 语义、不得改 gauntlet.config.json 与 .gauntlet）与 GAUNTLET.md 的质量现状；exclude 里 `tests/`/`**/tests/**` 的既有口径不变",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh --static（M6-03 段）；git diff 8601ac9 --stat -- gauntlet.config.json .gauntlet features/no_root_agent_config.feature",
    "M6-03 PASS（通过数 ≥59、没有被删的 #[test] 行）；gauntlet.config.json 与 .gauntlet/ 零改动；既有 feature 文件逐字节未动",
    "must-hold",
    "merge-m6.qa.md#M6-03",
    "blocker",
)

add(
    "MRG6-09",
    "M6 轮次的**范围守卫**成立：相对合流基线 `8601ac9` 的全部改动（含未跟踪文件）都落在 M6 允许集内——`crates/*`、`tests/*`、`Cargo.toml`、`Cargo.lock`、`deploy/*`（**除 `deploy/nginx.conf`**）、`README.md`、`docs/*`、`features/*`、`qa/*`、`demo/*`、`report/*`、`GAUNTLET.md`、`gauntlet-tools/*`；且冻结面 `web/`、`deploy/nginx.conf`、`.gauntlet/`、`gauntlet.config.json`、`gauntlet-baseline.json` 零改动。负例自检 5 条（`.gauntlet/gauntlet.mjs`、`gauntlet.config.json`、`gauntlet-baseline.json`、`web/src/main.tsx`、`deploy/nginx.conf`）必须逐条判越界；正例自检 2 条（`crates/server/src/handlers.rs`、`demo/*`）必须判允许。",
    "本轮任务书硬性约束 1/3 与「严格按 M6、不要扩大」；FIX-13/FIX-14/FIX-15 的允许集先例；qa/merge-m6.qa.md §8（F12 的轮次口径）",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/no-root-fixes-checks.sh --no-slow（F12 段）与 sh qa/harness/merge-m6-checks.sh --static（M6-04 段，独立实现）",
    "两处结论一致：越界集合为空、冻结面零改动、负例 5/5、正例 2/2；F12 判 PASS",
    "must-hold",
    "merge-m6.qa.md#M6-04",
    "blocker",
)

add(
    "MRG6-10",
    "刚落地的四项无 root 修复不得回归：`sh qa/harness/no-root-fixes-checks.sh` 的 F1–F12 必须全 PASS（12/12，证据 gauntlet-out/qa/evidence/no-root-fixes-checks.txt），其中 F1–F11 的口径与合流前完全一致；F12 按 M6 轮口径判（见 MRG6-09）。**F12 的轮次口径修正以追加本条的形式落账**：no-root 轮的旧口径仍可复跑（`NR_FIX_BASE=7ca587a`），且负例自检数量从 5 条**只增不减**（M6 轮另有 2 条正例自检）。",
    "本轮任务书硬性约束 2；FIX-14/FIX-15 的两次同类先例（spec 阶段修订 harness + 负例自检不放松 + 追加约束记录）；实测：未修脚本前本树是 PASS=11 FAIL=1（F12 唯一失败项是旧允许集没有 `report/*`）",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/no-root-fixes-checks.sh --no-slow；NR_FIX_BASE=7ca587a sh qa/harness/no-root-fixes-checks.sh --no-slow（旧口径对照）",
    "M6 轮口径：PASS=12 FAIL=0；旧口径：F1–F11 仍 PASS、F12 的两条范围断言 FAIL（原因写明是合流本身改了 server/storage，不是修复回归）",
    "must-hold",
    "merge-m6.qa.md#8",
    "blocker",
)

add(
    "MRG6-11",
    "文档一致性**只许变好**：`sh qa/harness/doc-claims-checks.sh` 的 FAIL 数不超过合流前的 **9** 条；其中 `DOC-CODE-SIGKILL-EXISTS` 与 `DOC-CODE-FORCE-OPTION` 必须由 FAIL 转 **PASS**（F-05 修好的机器判据）；TUI 快捷键 13 条、配置键 17 条必须保持全 PASS。",
    "report/merge-plan.md 的 M9 表（TUI 快捷键 / 配置键 / 文档一致性三行）；本轮任务书第 2 条「doc-claims-checks.sh 的 FAIL 数不增加」；qa/merge-m6.qa.md §1 第 5–7 行与 §5.1",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/doc-claims-checks.sh；sh qa/harness/merge-m6-checks.sh --static（M6-08/09/10 三段）",
    "FAIL ≤ 9 且 SIGKILL/force 两条为 PASS；DOC-TUI-KEY 13/13；DOC-*-YAML-KEY 17/17",
    "must-hold",
    "merge-m6.qa.md#M6-08",
    "major",
)

add(
    "MRG6-12",
    "审计覆盖面必须真的扩大（F-10）：经 REST 做一次登录、建任务、停任务、建用户（外加一次失败登录）之后，`audit_logs` 里新出现的 `action` 种类**不少于 3 种**，并且这些行能通过修好的 `GET /api/audit-logs` 读出来。测试/夹具只允许写自己造的前缀（`m6-`），**绝不** TRUNCATE 或清空共享表。",
    "report/merge-plan.md 的 M8（F-10「只有 2 个写入点」）与 M2（B-wip 的 13 处 insert_audit_log）；本轮任务书「审计覆盖 13 个写入点」；已知夹具卫生问题 N9（ops-checks.sh:112 清空 node_metrics）",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh（M6-12 段：记录 T0 → 做写操作 → select count(distinct action) from audit_logs where timestamp > T0）",
    "M6-12 PASS：新动作种类 ≥ 3，且审计端点能读到这些行；共享表未被整体清空",
    "must-hold",
    "merge-m6.qa.md#M6-12",
    "major",
)

add(
    "MRG6-13",
    "登录限速的**正判据**必须存在（F-08）：同一个来源地址连续 12 次失败登录里至少出现一次 **429**；同时 `qa/harness/extra-checks.sh` 的 `SEC-13-NO-IP-RATE-LIMIT`/`SEC-13-ALL-ATTEMPTS-401` **必然翻转为 FAIL**——那是它作为「合流前 finding 判据」的正常结局，不是回归。两条必须**成对存在**：只保留翻转记录而不给新正判据（或反之）都算不成立。",
    "report/merge-plan.md 的 M8（F-08）与 M2（B-wip 的按 IP 滑动窗口）；qa/qa-report.json 的 S12/SEC-13 条目；qa/merge-m6.qa.md §5.3 翻转登记表",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh（M6-13 段：12 次同源尝试的状态码序列）与 sh qa/harness/extra-checks.sh（SEC-13 两行）",
    "M6-13 PASS（429 ≥ 1 次）；extra-checks 的 SEC-13 两行 FAIL 且已在翻转登记表里写明理由",
    "must-hold",
    "merge-m6.qa.md#M6-13",
    "major",
)

add(
    "MRG6-14",
    "gRPC 任务生命周期与 WS 广播不得退化：`sh qa/harness/job-e2e.sh` 必须 0 FAIL（含 `JOB-SUCCEEDED`、`JOB-LOGS-CAPTURED`、`JOB-CANCELLED`、`JOB-PROC-GONE`、`ALERT-*` 与 `WS-CONNECTED/SUBSCRIBED/METRICS-PUSH/JOB-UPDATE-PUSH` 四条广播检查）；否则合流就是把 gRPC/WS 面做没了。",
    "report/merge-plan.md 的 M9（gRPC 服务方法 + WS 广播两行）；本轮任务书第 2 条；qa/merge-m6.qa.md §5.1",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/server-up.sh false && sh qa/harness/agent-up.sh qa-node-01 && sh qa/harness/job-e2e.sh; sh qa/harness/agent-down.sh; sh qa/harness/server-down.sh（merge-m6-checks.sh 的 M6-14 段自动完成这套起停）",
    "M6-14 PASS：PASS ≥ 10、FAIL = 0",
    "must-hold",
    "merge-m6.qa.md#M6-14",
    "major",
)

add(
    "MRG6-15",
    "迁移与 schema 的幂等性不得退化：重启 server 之后 `/api/health` = 200、`public` 下仍是 **11 张表**、`users` 里 `admin` 仍是 **1 行**（不重复插入）。",
    "report/merge-plan.md 的 M9（DB schema 一行，11 张表）；qa/merge-m6.qa.md §1 第 8 行与 §5.1",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh（M6-15 段：停下来再起一次，然后 psql 计数）；只读命令：psql -c '\\dt'、select count(*) from users where username='admin'",
    "M6-15 PASS：health=200、表数 11、admin 行数 1",
    "must-hold",
    "merge-m6.qa.md#M6-15",
    "major",
)

add(
    "MRG6-16",
    "QA harness 的**树定位**必须自解析，不许再把仓库根硬编码成 `.../ClusterScope-review/gh-line`：`qa/harness/env.sh` 用 `${REPO:-$HERE/../..}`（HERE/REPO 都没有时报错并返回非 0），`no-root-checks.sh` / `nr-verify.sh` / `nr-verify2.sh` / `nr-verify3.sh` 用 `R=\"${R:-$(cd \"$(dirname \"$0\")/../..\" && pwd)}\"`。这是**假证据风险**的修复（在 merge-m6 里跑旧脚本会静默地测 gh-line 那棵树），不改变任何一条断言。",
    "第 1 阶段实测：env.sh:5 与 no-root-checks.sh:13、nr-verify*.sh 都硬编码 gh-line；本轮 M9/M10 的验证必须落在合流后的树上",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/doc-claims-checks.sh（输出应与 qa/merge-m6.qa.md §1 第 5 行一致：71 PASS / 9 FAIL）；git diff 8601ac9 -- qa/harness/env.sh qa/harness/no-root-checks.sh qa/harness/nr-verify*.sh（只允许出现找根那几行与注释）",
    "两棵树的证据不再互相冒充；原有 CHECK/A 断言集合逐行不变；脚本语法 sh -n 通过",
    "must-hold",
    "merge-m6.qa.md#7",
    "major",
)

add(
    "MRG6-17",
    "F-12 的默认口令**本轮不改产品默认值**（`crates/common/src/config.rs` 里仍是 `default_admin_password: \"admin\"`），只允许做文档补强：README 的安全提示里写清「代码默认口令是 `admin`，首次启动后必须改」。并且**不得**改动 `doc-claims-checks.sh` 两条 grep 盯着的行（README 里的 `default_admin_password: \"admin123\"` 与代码里的 `default_admin_password: \"admin\"`），否则 FAIL 数会增加。",
    "report/merge-plan.md 的 M8（F-12「默认口令建议合流前就改文档/配置」）；qa/merge-m6.qa.md §9 的取舍记录（改默认值会牵动首启动守卫、示例配置、NR-07 与 no-root 检查）",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && grep -n 'default_admin_password' crates/common/src/config.rs README.md deploy/*.yaml.example; sh qa/harness/doc-claims-checks.sh（FAIL ≤ 9）",
    "代码默认值不变；README 有「必须改口令」的提示；doc-claims 的 FAIL 数不增加",
    "must-hold",
    "merge-m6.qa.md#9",
    "minor",
)

add(
    "MRG6-18",
    "**范围纪律**：本轮只做 M6 的步骤 1/2/3/5，步骤 4（`web/`、`deploy/nginx.conf`）不做；M7 的质量口径不动（不重算成 PASS、不还 97 项欠账、不开棘轮、不写 `quality-accepted.json`/`mutation-accepted.json`）；F-02（天级历史）与 F-07（read-only 鉴权边界）**本次不合入**，但必须按 qa/merge-m6.qa.md §9 写明取舍理由与后续路径；`qa/constraints.json` 只允许追加（既有 104 条 + FIX-01…FIX-14 一字不动，追加后 `git diff --numstat 7ca587a -- qa/constraints.json` 的第二列必须是 0）。",
    "本轮任务书的范围表与硬性约束 4/5；report/merge-plan.md 的 M7；qa/merge-m6.qa.md §8/§9",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && git diff --numstat 7ca587a -- qa/constraints.json（第二列 0）与 git diff --numstat 8601ac9 -- qa/constraints.json；sh qa/harness/no-root-fixes-checks.sh --no-slow（F11 段）；grep -n 'M7\\|F-02\\|F-07' qa/merge-m6.qa.md",
    "约束只追加（0 删行）；F-02/F-07 的取舍与后续路径写进 qa/merge-m6.qa.md；web 面零改动",
    "must-hold",
    "merge-m6.qa.md#9",
    "major",
)

add(
    "MRG6-19",
    "**共享机器纪律**：只按自己记录/自己 PID 文件停进程（`qa/harness/*-down.sh`），不用按名字整机匹配的 `pkill`/`killall`；**不碰**常驻 agent **PID 266643** 及其 systemd --user unit 与 `~/.config/clusterscope/` 配置；linger 保持 `yes` 不动；不 `git push`、不开 PR（推送与 PR 由 Leader 在用户通过后做）。",
    "本轮任务书硬性约束 7 与权限一节；qa/README.md 的三条硬规矩；GAUNTLET.md 的坑（本机常驻 agent）",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && ps -o pid,lstart,cmd -p 266643（仍在、启动时间不变）；loginctl show-user $USER -p Linger（=yes）；git log --oneline origin/master..HEAD --max-count=1 与 git status -sb（本地分支，无 push）",
    "PID 266643 存活且启动时间未变；Linger=yes；分支只存在于本地/远端仓库，没有推到 GitHub",
    "must-hold",
    "merge-m6.qa.md#0",
    "blocker",
)

add(
    "MRG6-20",
    "本轮的判据入口必须成立：`qa/harness/merge-m6-checks.sh`（M6-01…M6-15）在本轮结束时输出 **0 FAIL**（其中 M6-01…M6-10 为静态段，可直接用 `--static` 复跑；M6-11…M6-15 自己起停 server/agent，只按 PID 文件），证据落 `gauntlet-out/qa/evidence/merge-m6-*.txt` 并写进第 5 阶段的 `qa/qa-report.json`（`constraint: \"MRG6-xx\"`）。",
    "本轮任务书第 2/4 项（M9 的验证表就是合流后的判据）；gauntlet-specify 的 QA 模板（每条约束至少一条检查证实它）",
    "cd /public/tianyuyang/code/ClusterScope-review/merge-m6 && sh qa/harness/merge-m6-checks.sh",
    "`M6-CHECKS: PASS=15 FAIL=0`；每条 MRG6-* 在 qa-report.json 里都有配对条目",
    "must-hold",
    "merge-m6.qa.md#6",
    "blocker",
)


def main():
    with io.open(PATH, encoding="utf-8") as fh:
        text = fh.read()

    tail = "  }\n]"
    if not text.endswith(tail):
        raise SystemExit("unexpected tail of %s: %r" % (PATH, text[-40:]))

    ids = [c["id"] for c in json.loads(text)]
    before = len(ids)

    # 可选：第 2 个参数是「额外条目」的 JSON 数组（用于后续的勘误 / 修订）。
    # 纪律同前：只追加，**不改写**既有条目。
    if len(sys.argv) > 2:
        with io.open(sys.argv[2], encoding="utf-8") as fh:
            E.extend(json.load(fh))

    # 已经在文件里的 id 一律跳过 —— 本脚本可以重复执行（幂等）。
    seen = set(ids)
    todo = []
    for e in E:
        if e["id"] in seen:
            continue
        seen.add(e["id"])
        todo.append(e)

    if not todo:
        print("nothing to append (all ids already present in %s)" % PATH)
        return

    body = json.dumps(todo, ensure_ascii=False, indent=2)
    inner = body[body.index("\n") + 1 : body.rindex("\n")]      # drop the outer [ ]

    # 插在最后的 `  }` 与 `]` 之间：`  }` 行与 `]` 行逐字节不动 → diff 是纯追加。
    new_text = text[: -len("]")] + ",\n" + inner + "\n]"

    parsed = json.loads(new_text)                                # 语法自检
    assert len(parsed) == before + len(todo)
    assert len({c["id"] for c in parsed}) == len(parsed)

    with io.open(PATH, "w", encoding="utf-8", newline="") as fh:
        fh.write(new_text)

    print("appended %d constraints -> total %d" % (len(todo), len(parsed)))
    print("ids: %s" % ", ".join(e["id"] for e in todo))


if __name__ == "__main__":
    main()
