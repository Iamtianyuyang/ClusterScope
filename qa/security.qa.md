# QA：安全（S1–S17）

审查对象是**已发布线**，所以下面每条都是「对外可验证的行为」，不是设计评审意见。

## 前置（所有需要 server 的检查都从这段开始）

```sh
cd /public/tianyuyang/code/ClusterScope-review/gh-line
sh qa/harness/server-up.sh false      # 检查 PG（已就绪）→ 写 gauntlet-out/qa/server.yaml → 起 server → 等 /api/health
                                      # 监听 127.0.0.1:8080(REST/WS) + 127.0.0.1:50051(gRPC)
                                      # 配置：jwt_secret=qa-harness-secret-0123456789abcdef, admin/admin123, auth_required=false
sh qa/harness/agent-up.sh qa-node-01  # 起 agent（本机 6 张 L20），注册节点
...
sh qa/harness/agent-down.sh           # 只按 PID 文件停，绝不 pkill
sh qa/harness/server-down.sh
```

`qa/harness/env.sh` 里定义了 `$HTTP`（http://127.0.0.1:8080）、`$PGURL`、`psql_q`、`api_code`、`login` 等，脚本一律 `cd` 到仓库根再跑。
`jwt-mint.py` 用**已知的 harness 密钥**签发任意角色/任意 exp 的 token——这是本地夹具，用来测伪造/过期两条路径。

| # | 检查项 | 操作（确切命令） | 期望结果 | 判定 | 证实约束 |
|---|---|---|---|---|---|
| S1 | 一键安全矩阵 | `sh qa/harness/server-up.sh false && sh qa/harness/api-checks.sh 2>&1 \| tee gauntlet-out/qa/evidence/api-checks.txt; sh qa/harness/server-down.sh` | 20 条检查，实测 **18 PASS / 2 FAIL**（FAIL 见 D6、D7） | 其余 PASS | SEC-01…SEC-10 |
| S2 | read-only 边界 | 矩阵里的 `SEC-NODES-READONLY-OPEN`(200)、`SEC-JOB-POST-NOTOKEN`(401)、`SEC-USER-POST-NOTOKEN`(401)、`SEC-RULE-POST-NOTOKEN`(401)、`DOC-GET-USERS-READONLY`(实测 401，README:233 说 GET 开放 → FAIL) | 读开放、写要 token；**admin 级 GET（/api/users）在 read-only 下仍要 token**（比文档更严） | 写侧 PASS；文档侧 FAIL = 结论 | SEC-07、DOC-04 |
| S3 | 伪造 / 过期 / 垃圾 token | `FORGED=$(python3 qa/harness/jwt-mint.py wrong-secret admin 3600); EXPIRED=$(python3 qa/harness/jwt-mint.py qa-harness-secret-0123456789abcdef admin -3600); curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $FORGED" $HTTP/api/jobs; curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $EXPIRED" $HTTP/api/jobs; curl -s -o /dev/null -w '%{http_code}\n' -H 'Authorization: Bearer not-a-jwt' $HTTP/api/jobs` | `401` / `401` / `401` | 三条都必须是 401 | SEC-01、SEC-02、SEC-04 |
| S4 | 角色门 | `VIEWER=$(python3 qa/harness/jwt-mint.py qa-harness-secret-0123456789abcdef viewer 3600); curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $VIEWER" $HTTP/api/jobs; curl -s -o /dev/null -w '%{http_code}\n' -X POST -H "Authorization: Bearer $VIEWER" -H 'Content-Type: application/json' -d '{"node_id":"qa-node-01","name":"n","executable":"/bin/true","arguments":[],"working_directory":"/tmp"}' $HTTP/api/jobs` | `200` 与 `403` | 200 + 403 | SEC-03 |
| S5 | refresh token 轮换 | 见矩阵 `SEC-REFRESH-*`：登录取 refresh → 换一次 → 用**旧**的再换 → 用**新**的换 | 旧 `401`、新 `200` | 两条都符合 | SEC-05 |
| S6 | 登录锁定 | 矩阵里的 `SEC-LOGIN-LOCKOUT`：建专用用户 `qa-lock-*` → 连错 5 次 → 第 6 次用正确口令 | `429`（锁定 300s；admin 账号不受影响） | 429 | SEC-06 |
| S7 | `auth_required: true` 模式 | `sh qa/harness/auth-tui-checks.sh 2>&1 \| grep AUTHREQ`（脚本自己重启 server/agent） | `/api/health` 200；`/api/nodes` 无 token 401；登录 200；带 token 200；`/ws` 无 token 401 | 五条都符合 | SEC-08 |
| S8 | `agent_token` gRPC 认证 | `sh qa/harness/auth-tui-checks.sh 2>&1 \| grep AGENTTOKEN` | 错 token 的 agent 12s 内不出现在 `/api/nodes`（agent 日志有 6 条 register 失败）；对 token 的 12s 内出现 | 两条都符合 | SEC-09 |
| S9 | 建用户校验 | 矩阵里的 `SEC-CREATE-USER-*`：口令 5 字符 → 400；role=root → 400（重复用户名另测 → 409） | 400 / 400 / 409 | 符合 | SEC-10 |
| S10 | SQL 注入面 | `grep -rn 'format!' crates/storage/src/*.rs \| grep -iE 'select\|insert\|update\|delete'` | 只有 2 处：`audit_queries.rs:69`、`job_queries.rs:99`，都只拼 `$n` 占位符，值全部 `.bind()`；`queries.rs:175-186` 的表名来自固定白名单 | PASS：无可注入点 | SEC-11 |
| S11 | 命令注入面 | `grep -rn 'Command::new' crates/agent/src/*.rs` | `job_executor.rs:118` `Command::new(&executable)` 直接 exec（无 shell）；`metrics.rs:294,417,440` 调 `nvidia-smi` | PASS：无 shell 拼接 | SEC-12 |
| S12 | 登录无限速（IP 维度） | `for i in $(seq 1 20); do curl -s -o /dev/null -w '%{http_code}\n' -X POST -H 'Content-Type: application/json' -d '{"username":"no-such-user","password":"x"}' $HTTP/api/login; done \| sort \| uniq -c` | 实测 **20 次全 401，无 429** | **FAIL = 结论**：只有按账号锁定，没有 IP/全局限速（可无限枚举用户名、可放大 DoS） | SEC-13 |
| S13 | 令牌不可吊销 | `grep -rn 'jti' crates --include='*.rs' \| grep -v common/src/auth.rs; grep -rn 'blacklist\|revoked' crates/server/src/*.rs \| wc -l` | 0 / 0 | **FAIL = 结论**：access token 在 3600s 内即使账号被禁用/删除也有效（只验签 + exp） | SEC-14 |
| S14 | 认证事件无审计 | `grep -rn 'insert_audit_log' crates/server/src/*.rs` | 只有 2 个调用点：`handlers.rs:470`（create_job）、`handlers.rs:585`（stop_job） | **FAIL = 结论**：登录成功/失败、用户增删改都不写审计表 | SEC-15 |
| S15 | 口令存储与接口泄漏 | `sh qa/harness/server-up.sh false; T=$(curl -s -X POST -H 'Content-Type: application/json' -d '{"username":"admin","password":"admin123"}' $HTTP/api/login \| jq -r .access_token); curl -s -H "Authorization: Bearer $T" $HTTP/api/users \| jq -c '.[0] \| {username,password_hash}'; sh qa/harness/server-down.sh` | 实测 `{"username":"admin","password_hash":""}` | PASS：argon2 加盐哈希（`common/auth.rs:80-100`），列表接口的 `password_hash` 为空串（`models.rs:131` `#[sqlx(default)]`） | SEC-16 |
| S16 | 输入上限 | `T=…; curl -s -H "Authorization: Bearer $T" "$HTTP/api/jobs/x/logs?limit=0" \| jq length; curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $T" "$HTTP/api/jobs?page_size=99999"` | `limit=0` → 0 行（被 clamp 到 1）；`page_size=99999` → 200（被 clamp 到 200） | 符合（`handlers.rs:609-620,520-524`） | SEC-17 |
| S17 | 任务参数无内容校验 | `sed -n '425,435p' crates/server/src/handlers.rs` | 只有 `executable` 非空 + 节点存在两条校验 | **FAIL = 结论**：operator 可提交任意可执行文件/参数/环境变量（设计如此，但 README 未明示权限边界） | SEC-18 |

## 结论摘要（写报告时直接引用）

- **做得对的**：JWT 伪造/过期/垃圾 token 全部 401；角色门（viewer 不能写）403；refresh 轮换生效；账号锁定 429；`auth_required: true` 时 REST 与 WS 都要 token；`agent_token` 真的拦住了错误 token；SQL/命令注入面无缺口；口令用 argon2 且接口不回传哈希。
- **要写进报告的缺口**：登录无 IP 限速（S12）；token 不可吊销（S13）；审计只覆盖 2 个写操作（S14）；任务参数无校验（S17）；`GET /api/audit-logs` 直接 500（D6）；read-only 模式下 admin 级 GET 的行为与 README 描述不一致（S2/D7）。
