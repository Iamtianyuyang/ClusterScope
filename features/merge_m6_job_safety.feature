# 合流后的任务安全边界（F-11 参数上限、F-05 SIGTERM→SIGKILL 升级、F-06 死配置键）
# —— 全部来自 B 的 12 个未提交文件（M6 步骤 2），其中 SIGKILL 与参数上限在 C 线完全不存在。
#
# 场景名 ↔ Rust 测试名：见 features/merge_m6_audit_queries.feature 顶部的契约说明
#（小写 + 折叠空白的子串匹配，用 snake_case）。
#
# 驱动方式：
#   * 参数上限：`crates/server/src/handlers.rs` 的 `MAX_ARGS = 256` / `MAX_ARG_LEN = 4096` 与提交校验
#     （合流前 C 线只有“可执行文件非空 + 节点存在”两条校验）。若校验逻辑内联在 handler 里，
#     编码阶段可以把它抽成一个纯函数（`validate_job_request(&req) -> Result<(), …>`），handler 与测试共用。
#   * 取消升级：`crates/agent/src/job_executor.rs` 的 `JobRuntime`。它的 `pids` 是 `pub`，
#     测试可以直接登记 `(pid, starttime)` 后调用 `request_cancel`；也可以走完整 `execute_job` 路径。
#     进程一律用 `setsid()` 起成独立进程组（`pre_exec`），否则 `kill(-pid)` 会打到测试自己所在组。
#   * 配置键：`crates/common/src/config.rs` 的 `ServerConfig`。判据是“键真的被读到”，不是“字段存在于结构体里”。
#
# 覆盖的约束（qa/constraints.json）：MRG6-09 / MRG6-10 / MRG6-11 / MRG6-12。

功能: 合流后的任务参数上限、取消升级与配置键

  场景: a_job_submission_with_too_many_arguments_is_rejected
    假如 一次任务提交带了 257 个参数
    当 服务端校验这次提交
    那么 提交被拒绝，理由是参数个数超过上限
    而且 恰好 256 个参数的提交不会被这条规则拒绝

  场景: a_job_submission_with_an_argument_over_the_length_limit_is_rejected
    假如 一次任务提交里有一个长度 4097 的参数
    当 服务端校验这次提交
    那么 提交被拒绝，理由是单个参数过长
    而且 长度恰好 4096 的参数不会被这条规则拒绝

  场景: cancelling_a_job_whose_process_ignores_sigterm_escalates_to_sigkill
    假如 一个任务进程在独立进程组里运行、并且忽略 SIGTERM
    当 我请求取消这个任务
    那么 这个进程组会在宽限期（约 5 秒）之后被 SIGKILL 清掉
    # 合流前：C 线的 `request_cancel` 只发 SIGTERM，README:355 承诺的“force → SIGKILL”不存在（F-05）。

  场景: cancelling_a_job_that_exits_on_sigterm_does_not_wait_for_the_escalation
    假如 一个任务进程在独立进程组里运行、并且会按 SIGTERM 正常退出
    当 我请求取消这个任务
    那么 进程在 1 秒内就退出，不需要等到 SIGKILL 那一步
    而且 进程组的记录（pids）随后被清掉

  场景: the_configuration_keys_that_used_to_be_dead_are_read_from_the_file
    假如 一份配置文件把 "tls_enabled"、"prometheus_enabled"、"prometheus_addr" 与 "max_concurrent_ws_clients" 都写成非默认值
    当 服务端读入这份配置
    那么 这四个值都出现在读出来的配置里（与文件里的值一致）
    而且 没写的键仍然取默认值
    # 合流前：这 4 个键与另外 11 个键在 C 线是死键（F-06，共 15 个，其中 11 个进了清单）。

  场景: enabling_tls_without_certificate_or_key_paths_is_refused_with_a_clear_error
    假如 一份把 "tls_enabled" 打开、但没有给证书路径与私钥路径的配置
    当 服务端在启动前校验这份配置
    那么 校验以“缺少 tls_cert_path / tls_key_path”的明确错误失败
    而且 关掉 "tls_enabled" 后同样的配置能通过校验
