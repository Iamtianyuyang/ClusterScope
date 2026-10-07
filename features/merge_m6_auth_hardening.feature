# 合流后的登录限速与会话吊销（F-08 / F-09 / F-10 的一部分 —— 来自 B 的 12 个未提交文件，M6 步骤 2）
#
# 场景名 ↔ Rust 测试名：见 features/merge_m6_audit_queries.feature 顶部的契约说明
#（小写 + 折叠空白的子串匹配，用 snake_case，不要写成带空格的散文）。
#
# 驱动方式（两条 seam 都允许，选代价小的）：
#   * 限速器：`crates/server/src/handlers.rs` 的 `login_allowed(ip)` / `login_failed(ip)` /
#     `global_login_allowed()` / `record_global_login_attempt()` / `effective_client_ip(headers, addr, trust)`。
#     若它们仍需要完整 `AppState`，编码阶段可以把限速器抽成一个只含两个队列的小结构（handler 与测试共用），
#     抽的时候**不得**改变窗口与上限常量（10 次失败 / 60s per-IP；300 次 / 60s 全局）。
#   * 吊销与守卫：`crates/storage/src/user_queries.rs` 的 `revoke_all_refresh_tokens` / `consume_refresh_token` /
#     `add_refresh_token` / `delete_user_guarded`（真库，库地址与夹具卫生见上一个 feature 文件的说明；
#     测试用户一律用 `m6-` 前缀，绝不清理整表）。
#
# 覆盖的约束（qa/constraints.json）：MRG6-04 / MRG6-05 / MRG6-06 / MRG6-07。

功能: 合流后的登录限速与会话吊销

  场景: repeated_failed_logins_from_one_client_address_are_capped
    假如 客户端地址 "m6-ip-a" 已经失败登录 10 次
    当 同一个地址再发起第 11 次登录
    那么 这一次在到达口令校验之前就被拒绝
    # 合流前：C 线没有任何按来源的限速（`extra-checks.sh` 的 SEC-13 记录的就是这条 finding）。

  场景: the_login_attempt_cap_is_tracked_per_client_address
    假如 客户端地址 "m6-ip-a" 已经失败登录 10 次并因此被拒绝
    当 另一个客户端地址 "m6-ip-b" 发起登录
    那么 这个新地址的尝试仍然被允许

  场景: the_global_login_budget_bounds_attempts_across_all_client_addresses
    假如 整整 300 次登录尝试来自 300 个互不相同的客户端地址
    当 第 301 次尝试来自又一个新地址
    那么 这一次也被拒绝（全局预算挡住了分布式爆破）
    # 全局预算记的是**每一次**尝试（成功或失败），所以它比按地址的失败计数更早生效。

  场景: the_client_address_used_for_the_budget_follows_the_forwarded_header_when_the_proxy_is_trusted
    假如 一次请求来自套接字地址 "127.0.0.1:50000"，其 "X-Forwarded-For" 是 "203.0.113.9, 10.0.0.1"
    当 信任上游代理头时解析这次请求的客户端地址
    那么 得到的地址是 "203.0.113.9"
    而且 不信任代理头时得到的地址是 "127.0.0.1"

  场景: revoking_all_sessions_invalidates_every_outstanding_refresh_token_of_the_user
    假如 用户 "m6-revoke" 手里有一枚有效的 refresh 令牌
    当 管理员吊销该用户的全部会话
    那么 这枚令牌再也换不到新的访问令牌（消费结果为“已失效”）

  场景: a_refresh_token_can_only_be_consumed_once
    假如 用户 "m6-once" 手里有一枚有效的 refresh 令牌
    当 这枚令牌被消费一次之后再次被消费
    那么 第一次拿到该用户的身份
    而且 第二次什么也拿不到

  场景: refresh_tokens_are_stored_as_digests_so_the_raw_value_is_not_in_the_database
    假如 用户 "m6-digest" 拿到一枚新的 refresh 令牌
    当 我去数据库里查这枚令牌
    那么 库里的值不等于这枚原始令牌
    而且 库里存的是一串固定长度的十六进制摘要

  场景: deleting_a_user_also_deletes_its_refresh_tokens
    假如 用户 "m6-cascade" 有一枚有效的 refresh 令牌
    当 管理员删除这个用户
    那么 删除成功（不是外键约束报错）
    而且 这个用户的 refresh 令牌在库里一条都不剩

  场景: demoting_or_deleting_an_administrator_that_is_not_the_last_one_succeeds
    假如 库里除了既有的管理员之外，还有一个新造的启用管理员 "m6-admin-extra"
    当 管理员把它降级成 viewer、随后再删掉它
    那么 这两步都必须成功
    而且 既有的那个管理员账号没有被这次操作改动
    # 口径说明：B 的「最后一个启用管理员不得停用/删除」守卫（`update_user_guarded` / `delete_user_guarded`）
    # 需要「库里只剩一个启用管理员」的前置，而本机是**共享数据库**（既有 admin 必须一直在），
    # 所以它的**拒绝路径**不在自动化判据里跑（只能在专属 schema/库里做），只在 QA 文档里作为人工核验项记录；
    # 这里锁住的是它的**允许路径与既有保护**（不误伤、不越权改别人的账号）。
