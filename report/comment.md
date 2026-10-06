# ClusterScope 审查 · 证据包小结（第 6 阶段）

> **单文件证据包（先看这个）**：`report/review.html`
> **合流方案**：`report/merge-plan.md` · **使用教程**：`docs/review-howto.md`
> **审查对象**：GitHub 已发布线 `f9c080b`（TUI-only，40 个提交）· **报告提交**：`gauntlet/audit-gh-line` 的 `[report]` 提交

## 5 分钟审阅路线（入口）

1. **先看 3 处**（`report/review.html` 内）：① 第 1 节前 3 条 findings（F-01 / F-16 / F-02，都是"功能实际不可用"级别）；
   ② 第 2 节闸门面板（✅ 与 ❌ 分开呈现 + `verdict: pass` 的语义）；③ 第 5 节 N1–N9（审查自身的 9 处不精确）。
2. **再点 3 个演示**（`gauntlet-out/evidence/demos/`）：`04-rest-security-audit-logs.html`、`07-history-daily-tier-missing.html`、`02-quality-gates-fail.html`。
3. **最后看两处结构证据**：架构图 `gauntlet-out/evidence/diagrams/clusterscope-runtime.html`、合流方案 `report/merge-plan.md`（**M5 需产品决策**）。

## 结论（两句话）

- **证据包本身合格**：81 条约束逐条真跑、16 条 finding 有原始证据、8 个演示可回放、架构图通过 Archify 校验。
- **产品不健康**：硬阈值下 complexity 21/316（maxCC 23）、CRAP 45/316（maxCRAP 552）、覆盖率 20.7% 三闸门 ❌；
  外加两个"功能不可用"缺陷（审计端点恒 500、90 天天级历史永不返回）与四个安全边界缺口（无登录限速、token 不可吊销、审计只覆盖 2 个动作、任务参数无上限）。

## 16 条 findings 速览

| id | severity | 一句话 | B 线是否已修 |
|---|---|---|---|
| F-01 | major | `GET /api/audit-logs` 恒 500（`SELECT *` 与 `username` 列不匹配） | ✅ `eac070e` |
| F-16 | major | 审计 COUNT 语句零绑定（被 500 掩盖的第二个缺陷） | ✅ 同一个 `eac070e` |
| F-02 | major | 天级（90 天）历史永不返回、错误被静默吞 | ⚠️ 另一条修法，需比对 |
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

**合流前必修**：F-01 + F-16 + F-02 + F-07 ~ F-11 + F-12 里的默认口令。

## 合流一句话

以 **C（GitHub `f9c080b`）为基线**：先把 B 的 `eac070e` cherry-pick 进来（实测只有 4 个文件 / 6 个冲突块 / 约 140 行），
再按文件 graft B 的 12 个未提交改动（1784+/740−，不在任何提交或 bundle 里，**先打包校验**），
A 的独有资产（web 27 文件、`common/{dedup,metrics,sequence}.rs`、`tests/integration_test.rs`、`deploy/nginx.conf`）按 M5 的产品决策处理。
**不要整体 `git merge 19d8fbc`**（实测 35 个文件冲突，含 `modify/delete`）。

---

# 以下是 kit 自动生成的仪表盘与逐条明细（原始记录，未经修饰）

## ❌ Gauntlet 证据包：ClusterScope GitHub 已发布线（f9c080b）全维度审查

> ❌ **未通过：** 验收测试（0/0 通过）；函数质量（316 个函数，超标 21 个）；CRAP（最大 CRAP 552）；行覆盖率（20.7%）；QA 端到端（77/82 通过）；需求约束（76/81 条已证实）

> ⚠️ **需要你确认：** 改动了规则文件 gauntlet.config.json、qa/constraints.json；没有配置架构依赖检查（commands.arch）

```html title="闸门仪表盘"
<style>body{background:var(--background);color:var(--foreground);font-family:var(--font-sans);margin:0}</style><div style="padding:6px 4px"><div style="display:flex;gap:4px;flex-wrap:wrap;padding-bottom:10px;border-bottom:1px solid var(--border)"><div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid var(--chart-2);background:var(--chart-2);color:var(--background)">✓</div><div style="font-size:12px;font-weight:600">摸底</div><div style="font-size:11px;color:var(--muted-foreground)">commands 适配器</div></div><div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid var(--border);background:transparent;color:var(--background)"></div><div style="font-size:12px;font-weight:600">规格</div><div style="font-size:11px;color:var(--muted-foreground)">0 个 Gherkin 场景</div></div><div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid color-mix(in srgb, #dc2626 85%, var(--foreground));background:color-mix(in srgb, #dc2626 85%, var(--foreground));color:var(--background)">✗</div><div style="font-size:12px;font-weight:600">编码</div><div style="font-size:11px;color:var(--muted-foreground)">44/44 个测试通过</div></div><div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid color-mix(in srgb, #dc2626 85%, var(--foreground));background:color-mix(in srgb, #dc2626 85%, var(--foreground));color:var(--background)">✗</div><div style="font-size:12px;font-weight:600">清理</div><div style="font-size:11px;color:var(--muted-foreground)">最大圈复杂度 23，重复 0%</div></div><div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid color-mix(in srgb, #dc2626 85%, var(--foreground));background:color-mix(in srgb, #dc2626 85%, var(--foreground));color:var(--background)">✗</div><div style="font-size:12px;font-weight:600">加固</div><div style="font-size:11px;color:var(--muted-foreground)">未运行</div></div><div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid color-mix(in srgb, #dc2626 85%, var(--foreground));background:color-mix(in srgb, #dc2626 85%, var(--foreground));color:var(--background)">✗</div><div style="font-size:12px;font-weight:600">QA</div><div style="font-size:11px;color:var(--muted-foreground)">77/82 项检查通过</div></div><div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid color-mix(in srgb, #dc2626 85%, var(--foreground));background:color-mix(in srgb, #dc2626 85%, var(--foreground));color:var(--background)">✗</div><div style="font-size:12px;font-weight:600">证据包</div><div style="font-size:11px;color:var(--muted-foreground)">本页 + 架构图</div></div></div><div style="margin:8px 0"><div style="display:flex;justify-content:space-between;font-size:12px"><span>✓ 测量范围（静态分析）</span><span style="font-family:ui-monospace,monospace;color:var(--chart-2)">100% · 下限 100%</span></div><div style="position:relative;height:10px;border:1px solid var(--border);border-radius:3px;background:var(--muted);margin-top:3px"><div style="position:absolute;left:0;top:0;bottom:0;width:100.0%;background:var(--chart-2);opacity:.35;border-right:2px solid var(--chart-2)"></div><div style="position:absolute;top:-4px;bottom:-4px;left:100.0%;border-left:2px dashed var(--muted-foreground)"></div></div></div><div style="margin:8px 0"><div style="display:flex;justify-content:space-between;font-size:12px"><span>✗ 最大圈复杂度（全部函数）</span><span style="font-family:ui-monospace,monospace;color:color-mix(in srgb, #dc2626 85%, var(--foreground))">23 · 上限 10</span></div><div style="position:relative;height:10px;border:1px solid var(--border);border-radius:3px;background:var(--muted);margin-top:3px"><div style="position:absolute;left:0;top:0;bottom:0;width:80.0%;background:color-mix(in srgb, #dc2626 85%, var(--foreground));opacity:.35;border-right:2px solid color-mix(in srgb, #dc2626 85%, var(--foreground))"></div><div style="position:absolute;top:-4px;bottom:-4px;left:34.8%;border-left:2px dashed var(--muted-foreground)"></div></div></div><div style="margin:8px 0"><div style="display:flex;justify-content:space-between;font-size:12px"><span>✓ 编译告警</span><span style="font-family:ui-monospace,monospace;color:var(--chart-2)">0 · 上限 0</span></div><div style="position:relative;height:10px;border:1px solid var(--border);border-radius:3px;background:var(--muted);margin-top:3px"><div style="position:absolute;left:0;top:0;bottom:0;width:0.0%;background:var(--chart-2);opacity:.35;border-right:2px solid var(--chart-2)"></div><div style="position:absolute;top:-4px;bottom:-4px;left:0.0%;border-left:2px dashed var(--muted-foreground)"></div></div></div><div style="margin:8px 0"><div style="display:flex;justify-content:space-between;font-size:12px"><span>✓ 重复代码</span><span style="font-family:ui-monospace,monospace;color:var(--chart-2)">0% · 上限 3%</span></div><div style="position:relative;height:10px;border:1px solid var(--border);border-radius:3px;background:var(--muted);margin-top:3px"><div style="position:absolute;left:0;top:0;bottom:0;width:0.0%;background:var(--chart-2);opacity:.35;border-right:2px solid var(--chart-2)"></div><div style="position:absolute;top:-4px;bottom:-4px;left:30.0%;border-left:2px dashed var(--muted-foreground)"></div></div></div><div style="margin:8px 0"><div style="display:flex;justify-content:space-between;font-size:12px"><span>✗ 需求约束</span><span style="font-family:ui-monospace,monospace;color:color-mix(in srgb, #dc2626 85%, var(--foreground))">76/81 · 下限 81/81</span></div><div style="position:relative;height:10px;border:1px solid var(--border);border-radius:3px;background:var(--muted);margin-top:3px"><div style="position:absolute;left:0;top:0;bottom:0;width:93.8%;background:color-mix(in srgb, #dc2626 85%, var(--foreground));opacity:.35;border-right:2px solid color-mix(in srgb, #dc2626 85%, var(--foreground))"></div><div style="position:absolute;top:-4px;bottom:-4px;left:100.0%;border-left:2px dashed var(--muted-foreground)"></div></div></div></div>
```

- 📏 产品代码 33 个文件、7030 行：静态分析 100%，有覆盖率数据 100%，被变异测试触及 0%。
- 🔬 316 个函数：最大圈复杂度 23，最长 196 行，最深嵌套 7 层；编译告警 0 条。
- 📋 重复代码占 0%（0 处，其中跨目录 0 处）。
- 📦 本分支改动 87 个文件，+19525 / −0 行，其中 2 个是规则文件。

### 📌 需求约束 76/81
- ✅ **GATE-01** 真实构建闸门成立：`cargo build --workspace --all-targets --offline` 退出码 0（三棵树里本线是可构建的）。
- ✅ **GATE-02** 真实测试闸门成立：44 个测试全部通过、0 失败、0 忽略。
- ✅ **GATE-03** clippy 与 rustfmt 闸门成立：`cargo clippy --workspace --all-targets --offline` 与 `cargo fmt --all --check` 都是 0 发现。
- ✅ **GATE-04** 重复代码与静态测量范围闸门成立：duplication 0.0%、scope 33/33 文件被解析。
- ✅ **GATE-05** 复杂度闸门在硬阈值下失败：316 个函数中 21 个超标，最大圈复杂度 23（阈值 10）。
- ✅ **GATE-06** CRAP 闸门在硬阈值下失败：45 个函数超标，最大 CRAP 552（阈值 8），其中 44 项覆盖率 0%。
- ✅ **GATE-07** 行覆盖率闸门在硬阈值下失败：总体 20.7%（1381/6686，阈值 90%），storage crate 0.0%。
- ✅ **GATE-08** 架构闸门处于「跳过」状态：gauntlet.config.json 没有 commands.arch，模块依赖方向没有任何自动检查。
- ✅ **GATE-09** 仓库没有任何 CI：`.github/` 不存在，README:344-348 的三条闸门只能人工手跑。
- ✅ **GATE-10** `node .gauntlet/gauntlet.mjs test` 的退出码 1 只来自 ACCEPTANCE（features/ 为空），不是构建/测试坏了。
- ✅ **GATE-11** 三个交付物二进制存在且是 release 构建（README:76 承诺 target/release/clusterscope-{agent,server,tui}）。
- ✅ **GATE-12** 覆盖率机制本身可用：rust-gate.mjs 能产出 junit.xml 与 lcov.info（无 cargo-llvm-cov 的替代实现）。
- ✅ **DOC-01** 仓库缺少 LICENSE 文件：README:19 徽章与 README:363 链接指向 blob/master/LICENSE（404），Cargo.toml:14 却声明 Apache-2.0。
- ✅ **DOC-02** docs/architecture.md:62 的保留策略「2s → 1min → 10min」与实现不符；实现是原始 24h + 小时 7d + 天 90d。
- ✅ **DOC-03** `clusterscope-server --help` 必须打印帮助并退出 0（agent/tui 都能）；实际是 `Error: Config file not found: --help` 退出 1，尽管依赖了 clap 却手读 argv。
- ✅ **DOC-04** docs/api.md:5 声称除 health/login/refresh-token 外全部端点都要 Bearer JWT；README:233 声称 auth_required:false 时 GET 免密。两者互斥，实际以后者为准。
- ✅ **DOC-05** docs/api.md 的端点表不完整：实际路由 /api/users/{id}(GET/PATCH/DELETE)、/api/alerts/rules/{rule_id}/state、/api/prometheus/metrics、/api/health 都没写进文档。
- ✅ **DOC-06** `GET /api/audit-logs`（docs/api.md:84 记录为可用）实际返回 500：查询用 `SELECT *`，而模型字段是 `user`、表列是 `username`。
- ✅ **DOC-07** README:307/357 与 docs/api.md:36-40 承诺历史查询合并三档粒度（含天级 90 天）；实际天级永不返回（DATE 列解码失败，错误被 `if let Ok` 吞掉）。
- ✅ **DOC-08** README:358 声称 cluster/info 的 idle_gpus / avg_gpu_utilization / active_alerts 无数据时为 null；实际 active_alerts 恒为 0（`unwrap_or(0)`），另两个才是 null。
- ✅ **DOC-09** README:355 声称任务取消「可配 force 后升级为 SIGKILL」；代码里没有任何 SIGKILL 或 force 选项（只有 SIGTERM 打进程组）。
- ✅ **DOC-10** deploy/agent.yaml.example:20,23,28 与 README:246 文档化的 log_level / disk_mounts / collect_process_details 三个键在代码里从未被读取（写了也不生效）。
- ✅ **DOC-11** server 侧同样有一批死配置键：redis_url、prometheus_enabled、prometheus_addr、ws_heartbeat_interval_secs、ws_slow_threshold_ms、ws_max_backlog、max_concurrent_ws_clients、tls_enabled 全部只在 config.rs 里出现。
- ✅ **DOC-12** README:232 与 deploy/server.yaml.example:13 用 default_admin_password: admin123，而 serde 默认值是 "admin"（crates/common/src/config.rs:120）——省略该键时初始口令与文档不同。
- ✅ **DOC-13** README:356 说 Web 前端已移除，但 server 仍注册 /ws，docs/api.md:86-95 也仍文档化 WebSocket。
- ✅ **DOC-14** README:217 承诺指标 2s 一次；实测 20 秒窗口内 node_metrics 新增约 10 行（2s 节拍）。
- ✅ **DOC-15** README:296 承诺 Server 重启后 Agent 每 60s 自动重新注册；实测重启后 47s 节点重新出现在 /api/nodes。
- ✅ **DOC-16** README:154-160 的 TUI 快捷键（j/k/p/h/l/Tab/1-4/r/?/q）与 crates/tui/src/main.rs:85-140 的实现逐键对应。
- ✅ **DOC-17** README:117-125 的 TUI 参数与默认值（-s http://127.0.0.1:8080、-u/-p、-i 3）与 clap 定义一致，`--help` 可用。
- ✅ **DOC-18** README:57 要求 PostgreSQL v16+，deploy/docker-compose.yml:5 用 postgres:16-alpine；本机 PostgreSQL 16.4 可连、server 可跑。
- ✅ **DOC-19** README:353 声明 gRPC 未启用 TLS、tls_enabled 只是预留；代码确实从不读取该键。
- ✅ **DOC-20** README:166 承诺 Top CPU 进程取 15 个、首个周期不产生数据、采不到不伪造 0；代码常量 TOP_CPU_PROCESSES=15 且首扫只建基线。
- ✅ **DOC-21** README:288-291 用 `systemctl --user` 管理服务，但 deploy/server.service、deploy/agent.service 是系统级 unit（User=clusterscope、路径 /usr/local/bin、/etc/clusterscope）——两套部署方式并存且未说明差异。
- ✅ **DOC-22** deploy/Dockerfile.server:9 EXPOSE 8081 9090，但 server 只监听 8080/50051（prometheus_addr 未被读取）——容器端口声明与实际不符。
- ✅ **SEC-01** 伪造 JWT（错误密钥签名）必须被拒：GET /api/jobs → 401。
- ✅ **SEC-02** 过期 JWT 必须被拒：exp 已过的同密钥 token → 401。
- ✅ **SEC-03** 合法 viewer token 只能读：GET /api/jobs → 200，POST /api/jobs → 403。
- ✅ **SEC-04** 垃圾 token / 空 Bearer 处理正确：非 JWT 字符串 → 401。
- ✅ **SEC-05** refresh token 轮换生效：用过的 refresh token 再换 → 401，新 refresh token 可用 → 200。
- ✅ **SEC-06** 登录失败 5 次后账号锁定：第 6 次（即使口令正确）返回 429。
- ✅ **SEC-07** read-only 模式下写操作仍需 token：POST /api/jobs、/api/users、/api/alerts/rules 无 token → 401。
- ✅ **SEC-08** auth_required: true 时 GET 也必须带 token：/api/nodes 无 token → 401，带 token → 200；/ws 无 token → 401。
- ✅ **SEC-09** agent_token 生效：server 配了 token 后，token 不符的 agent 无法注册，token 相符的可以。
- ✅ **SEC-10** 用户创建有最低校验：口令 <6 字符 → 400，role 不在 viewer/operator/admin → 400；重复用户名 → 409。
- ✅ **SEC-11** SQL 注入面收敛：所有用户输入都走绑定参数，`format!` 只用于拼接参数占位符与固定白名单表名。
- ✅ **SEC-12** 命令注入面收敛：agent 用 `Command::new(executable).args(...)` 直接 exec，不经 shell；任务自身可以显式跑 /bin/sh（那是任务内容，不是注入）。
- ✅ **SEC-13** 登录接口没有 IP/全局限速：对不存在的用户名连续 20 次错误登录不会触发 429（只有按账号的锁定）。
- ✅ **SEC-14** 访问令牌不可吊销：JWT 无黑名单/jti 校验，用户被删除或禁用后其已签发的 access token 在有效期内仍可用（默认 3600s）。
- ✅ **SEC-15** 认证事件没有审计：insert_audit_log 只在 create_job / stop_job 调用，登录成功/失败、用户增删改都不写审计表。
- ✅ **SEC-16** 密码存储用 argon2 默认参数加盐哈希，接口不返回哈希（GET /api/users 返回的 password_hash 为空串）。
- ✅ **SEC-17** 任务提交的输入上限存在：`/api/jobs/{id}/logs` 的 limit 被 clamp 到 1..10000，list 接口 page_size clamp 1..200。
- ✅ **SEC-18** job 提交的 executable/arguments 无长度与内容校验，只有「非空」检查；超长/绝对路径/环境变量注入都由 agent 原样执行。
- ✅ **CON-01** 任务生命周期端到端成立：POST /api/jobs → queued → starting → running → succeeded，日志按 offset 入库并可查。
- ✅ **CON-02** 取消任务真的杀掉进程组：DELETE /api/jobs/{id} → stopping → cancelled，且 sleep 300 子进程不残留。
- ✅ **CON-03** 停止终态任务被拒：对 succeeded 任务 DELETE → 409；未知 job_id → 404。
- ✅ **CON-04** 卡在 starting 且节点不在线的任务会被 requeue 回 queued，并清空 started_at（避免双跑/容量泄漏）。
- ✅ **CON-05** GPU 容量感知调度成立：6 卡节点上两个 gpu:6 任务只有一个 running，另一个排队；取消第一个后第二个被派发。
- ✅ **CON-06** 告警去重成立：同一 (rule,node,gpu) 在持续越限期间不再产生新事件（10s 内 5 次上报，事件数不变）。
- ✅ **CON-07** 任务 pid 从未落库：jobs.pid 在所有任务行上都是 NULL，尽管 agent 日志记录了真实 pid。
- ✅ **CON-08** 重试机制不存在：retry_count/max_retries 只是列，任何代码路径都不会写非 0 值（失败任务不会被重试）。
- ✅ **CON-09** 原始指标保留 24h 生效：写入 25h 前的行会在 ≤20s 内被后台循环删除，1h 前的行保留。
- ✅ **CON-10** 小时级/天级清理与任务日志保留按文档节拍执行（7 天 / 90 天 / 30 天），但都挂在 10 分钟倍数 tick 上：清理最坏延迟 10 分钟。
- ✅ **CON-11** 聚合是幂等的（ON CONFLICT upsert），但只在每 10 分钟 tick 跑：新建节点的小时桶最坏 10 分钟后才出现。
- ✅ **CON-12** 历史查询的三档合并：原始 24h + 小时 7d 都返回并带 source 标记、按时间升序；天级（90 天）永不返回（缺陷，见 DOC-07）。
- ✅ **CON-13** 迁移可重复执行：连续两次启动 server 都健康，admin 用户不会重复创建。
- ✅ **CON-14** WebSocket 广播仍然工作：连接后收到 connected/subscribed，并持续收到 metrics_update，任务状态变化有 job_update。
- ✅ **CON-15** gRPC 流不会泄漏：agent 断开后 get_pending_jobs 的 5s 轮询循环因 send 失败退出；report_metrics 每条上报一个独立流。
- ✅ **CON-16** 去重缓存有界但无过期：seen_reports 是 100000 条 LRU（(node,seq)），长时间运行不会无界增长。
- ✅ **OPS-01** server 只监听配置里的 8080/50051；配置键 prometheus_addr（默认 0.0.0.0:9090）没有任何监听者。
- ✅ **OPS-02** /api/prometheus/metrics 从 REST 端口暴露两个指标（nodes_total、nodes_online），没有独立 exporter。
- ✅ **OPS-03** systemd unit 里的路径与参数有效：agent 用 `--config`（clap 定义存在）、server 用位置参数（argv[1] 解析存在）。
- ✅ **OPS-04** deploy/install-agent.sh 语法与用法成立：`bash -n` 通过、参数顺序与 README:92-93 一致、node_id 为空时走 hostname 分支。
- ❌ **OPS-05** docker-compose 那条路在本机不可验证（无 docker/podman 镜像、无外网），只能做静态一致性检查：镜像版本、env 键、构建上下文。
- ✅ **OPS-06** server.yaml.example / agent.yaml.example 的每个键都能被 serde 反序列化（不会因未知键报错或静默忽略）。
- ✅ **OPS-07** 没有数据库迁移工具链：建表脚本内联在 DatabasePool::run_migrations，migrations.rs 是只有注释的占位模块，无法回滚/审计 schema 版本。
- ✅ **OPS-08** 本机部署可用性成立：无 root 也能跑 server + agent + TUI（PG 16.4 手装实例、TUI 在 pty 里正常渲染）。
- ❌ **OPS-09** agent 在无 GPU 的机器上也能正常上报（纯 CPU 节点）——README:323 承诺的降级路径；本机有 6 张 L20，属于更强条件，未覆盖纯 CPU 场景。
- ❌ **OPS-10** README:297 要求防火墙放行 50051/8080——本机无 root，防火墙规则不可验证；只能确认两个端口确实在监听。
- ✅ **OPS-11** TUI 依赖 REST 且不链接内部 crate（README:15、架构图 TUI→REST）：`cargo tree -p tui` 只应出现 common/protocol 之外的外部依赖。
- ❌ **FE-01** 前端维度在本线不适用（N/A）：审查对象 f9c080b 已删除 `web/`，仓库里没有任何前端代码（无 `web/`、无 `package.json`、无 html/ts/js）；前端只存在于另两棵树（A 的 `local-wip/web/` 有 27 个文件：App.tsx / auth.ts / i18n.tsx / pages / services）。
- ❌ **MRG-01** 合流方案必须逐条回答 qa/merge-plan-requirements.md 里的必答问题（三棵树的差异、重复实现、被丢弃的工作、冲突点、推荐路线、风险）。

### 📘 验收场景 0/0

> 完整报告见附件 `index.html`（含 F 面板架构图、逐文件测量范围、代码质量明细、演示录像）。架构图单独附件见 `diagrams/*.html`。评论时写面板字母即可定位。
