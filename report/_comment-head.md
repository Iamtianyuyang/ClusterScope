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

