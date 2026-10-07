# 接回 A 的独有资产（M6 步骤 3，按 M5 裁决先 TUI-only）：
#   `crates/common/src/metrics.rs`、`crates/storage/src/conversions.rs`，
#   以及 A 的 `tests/integration_test.rs` 里仍然成立的行为。
#
# 场景名 ↔ Rust 测试名：见 features/merge_m6_audit_queries.feature 顶部的契约说明
#（小写 + 折叠空白的子串匹配，用 snake_case）。
#
# 重要口径（M3 的裁决，不要读成“删测试”）：
#   A 的 `tests/integration_test.rs` 里有两条**依赖 A 独有模块**的用例（`common::dedup`、`common::sequence`）——
#   那两个模块在 M3 里判定为「被 C 的 `LruCache` / `AtomicU64` 方案替代，不捡回」，
#   所以移植这个文件时**不**移植这两条用例（它们对应的产品代码本来就不进主线）。
#   其余用例（节点注册表、告警状态机、任务状态机、指标聚合、口令与 JWT、角色权限）按 C 的 API 改签名后原样移植，
#   它们**只能增加**测试数，不得删掉 C 线已有的任何测试。
#
# 驱动方式：
#   * `common::metrics::MetricsAggregation::new(name, &values, start_ms, end_ms)` 是纯函数，直接断言。
#   * `storage::conversions::node_metrics_to_proto(&NodeMetricsRow)` 是纯函数；`NodeMetricsRow` 的字段以
#     `crates/storage/src/models.rs` 合流后的定义为准（A 的版本是 SQLite 时代的，签名要按 C 改）。
#   * 不要求测试连库：这两条都是进程内的类型行为。
#
# 覆盖的约束（qa/constraints.json）：MRG6-13 / MRG6-14。

功能: 接回 A 的独有指标类型与转换

  场景: metric_aggregation_reports_average_extremes_and_sample_count
    假如 一组样本 10、20、30、40、50、60、70、80、90、100
    当 我按这组样本构造一次指标聚合
    那么 平均是 55、最大是 100、最小是 10、样本数是 10
    而且 百分位 p95 落在这组样本的最小值与最大值之间

  场景: metric_aggregation_of_an_empty_sample_is_zeroed_with_count_zero
    假如 一组空样本
    当 我按空样本构造一次指标聚合
    那么 平均、最大、最小、百分位都是 0，样本数是 0
    而且 起始与结束时间戳原样保留

  场景: a_stored_metrics_row_converts_to_the_protocol_report_without_losing_identity
    假如 一条存库的节点指标记录，node_id 为 "m6-node"，sequence 为 42，时间戳为 1700000000000
    当 我把它转换成上报协议里的指标报告
    那么 报告里的 node_id、sequence、时间戳与源记录逐一对齐
    而且 CPU、负载与内存字段与源记录里的值一致

  场景: missing_optional_values_in_a_stored_row_convert_to_zero_instead_of_failing
    假如 一条存库的节点指标记录里 CPU、负载与内存字段都是空值
    当 我把它转换成上报协议里的指标报告
    那么 转换照常成功
    而且 这些缺失的数值字段一律变成 0
