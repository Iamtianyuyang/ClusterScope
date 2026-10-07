# 合流后的审计日志查询正确性（F-01 + F-16 的合流修法 = M6 步骤 1 的 cherry-pick `eac070e`）
#
# 场景名 ↔ Rust 测试名的硬契约（commands 适配器；`.gauntlet/lib/adapter-commands.mjs` 的 matchAcceptance）：
#   闸门把「场景名」与「每个测试的 JUnit 名」都转小写、折叠空白后做**子串**匹配（`_` ≠ 空格），
#   所以这里的场景名整体写成 snake_case，编码阶段照这个名字写测试函数：
#     场景: audit_listing_filters_rows_and_returns_a_matching_total
#     ⇔ Rust: fn audit_listing_filters_rows_and_returns_a_matching_total()
#
# 驱动方式（首选真库 —— 本机 PG 16.4 已就绪，见 GAUNTLET.md「PostgreSQL 16.4」）：
#   * 库地址：`POSTGRES_URL`，未设时用 `postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`；
#     表结构来自 `crates/storage/src/lib.rs` 的 schema / 迁移（11 张表之一 `audit_logs`）。
#   * seam：`storage::audit_queries::{insert_audit_log, list_audit_logs}`（合流后的公开 API，签名不变）。
#   * 夹具卫生（M6 硬约束 MRG6-08）：测试只写、只清自己造的 username/action 前缀（统一用 `m6-`），
#     **绝不** TRUNCATE / DELETE 整表 —— 这台机器上的 PG 是共享的（已知坑：ops-checks.sh:112 会清空 node_metrics）。
#   * 判据用的是「返回条数 + total 一致」而不是 SQL 文本：这一条同时守住 F-01（占位符 $n 整体错位）
#     与 F-16（COUNT 语句一个参数都没绑）——两者都会让这个端点在合流前恒 500。
#
# 覆盖的约束（qa/constraints.json）：MRG6-01 / MRG6-02 / MRG6-03。

功能: 合流后的审计日志查询

  背景:
    假如 一张建好的审计日志表，里面有 3 条属于用户 "m6-audit" 的记录，其中 action 为 "m6-login" 的有 2 条、为 "m6-create" 的有 1 条
    而且 还有 1 条属于用户 "m6-other" 的记录

  场景: audit_listing_filters_rows_and_returns_a_matching_total
    当 我按用户 "m6-audit" 查询审计日志
    那么 返回 3 条记录
    而且 返回的总数是 3
    而且 返回的记录里没有任何一条属于用户 "m6-other"

  场景: audit_listing_returns_zero_total_instead_of_an_error_when_nothing_matches
    当 我按用户 "m6-nobody" 查询审计日志
    那么 返回 0 条记录
    而且 返回的总数是 0（不是错误，也不是空值）

  场景: audit_listing_is_ordered_newest_first_and_pages_by_offset
    假如 这 3 条 "m6-audit" 记录的 timestamp 依次递增
    当 我按每页 2 条依次取第 1 页与第 2 页
    那么 第 1 页是时间最新的 2 条
    而且 第 2 页只剩 1 条（也就是最早的那条）
    而且 两页合起来恰好是全部 3 条、没有重复

  场景: audit_listing_combines_user_action_and_time_filters
    当 我同时按用户 "m6-audit"、action "m6-login" 与覆盖全部 3 条记录的时间窗口查询
    那么 返回 2 条记录，总数也是 2
    而且 把时间窗口收紧到只覆盖最早那条记录的更早时段时，返回 0 条、总数是 0
