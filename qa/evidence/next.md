# gauntlet next — quality — 第 1 轮

结论：**CONTINUE**　剩余 97 项，离阈值的距离 409.505　失败的闸门：complexity, crap, coverage

下一步：修第 1 项（同一闸门的同类问题可以一起修），然后重新运行本命令。没有轮数上限：只要指标还在向阈值靠近就继续。

## complexity（21 项）— 提取函数 / 卫语句 / 表驱动 / 参数对象，把指标降到阈值内
1. `crates/tui/src/ui.rs:599` node_panel：圈复杂度 23 > 10；长度 170 行 > 60
2. `crates/tui/src/ui.rs:1024` draw_process：圈复杂度 18 > 10；长度 196 行 > 60
3. `crates/server/src/main.rs:383` run_scheduler_cycle：圈复杂度 15 > 10；长度 110 行 > 60
4. `crates/common/src/alert.rs:140` evaluate：圈复杂度 12 > 10；长度 135 行 > 60；嵌套 5 层 > 4
5. `crates/server/src/main.rs:314` run_background_tasks：圈复杂度 12 > 10；长度 66 行 > 60
6. `crates/tui/src/ui.rs:1223` draw_cpu_processes：长度 109 行 > 60
7. `crates/server/src/grpc.rs:602` evaluate_alerts：长度 63 行 > 60；嵌套 7 层 > 4
8. `crates/tui/src/ui.rs:781` draw_trend_full：长度 114 行 > 60
9. `crates/server/src/grpc.rs:141` report_metrics：长度 110 行 > 60
10. `crates/server/src/handlers.rs:217` get_metrics_history：长度 75 行 > 60
11. `crates/tui/src/ui.rs:463` draw_topbar：长度 62 行 > 60
12. `crates/server/src/handlers.rs:631` create_alert_rule：长度 69 行 > 60
13. `crates/server/src/handlers.rs:31` login：长度 65 行 > 60
14. `crates/server/src/grpc.rs:451` update_job_status：长度 69 行 > 60
15. `crates/tui/src/ui.rs:1432` draw_alerts：长度 96 行 > 60
16. `crates/server/src/main.rs:234` build_http_router：长度 79 行 > 60
17. `crates/server/src/handlers.rs:420` create_job：长度 71 行 > 60
18. `crates/server/src/grpc.rs:252` submit_job：长度 64 行 > 60
19. `crates/server/src/ws_handler.rs:154` handle：嵌套 6 层 > 4
20. `crates/server/src/grpc.rs:320` get_pending_jobs：嵌套 6 层 > 4
21. `crates/storage/src/audit_queries.rs:8` insert_audit_log：参数 9 个 > 7

## crap（45 项）— 降低复杂度，或为该函数补测试
22. `crates/tui/src/ui.rs:599` node_panel：CRAP 552.0 > 8；complexity 23 > 10（覆盖率 0%）
23. `crates/tui/src/ui.rs:1024` draw_process：CRAP 342.0 > 8；complexity 18 > 10（覆盖率 0%）
24. `crates/server/src/main.rs:383` run_scheduler_cycle：CRAP 240.0 > 8；complexity 15 > 10（覆盖率 0%）
25. `crates/server/src/main.rs:314` run_background_tasks：CRAP 156.0 > 8；complexity 12 > 10（覆盖率 0%）
26. `crates/tui/src/ui.rs:1223` draw_cpu_processes：CRAP 90.0 > 8（覆盖率 0%）
27. `crates/server/src/grpc.rs:602` evaluate_alerts：CRAP 90.0 > 8（覆盖率 0%）
28. `crates/agent/src/config_loader.rs:5` load_config：CRAP 90.0 > 8（覆盖率 0%）
29. `crates/server/src/main.rs:181` load_config：CRAP 72.0 > 8（覆盖率 0%）
30. `crates/server/src/auth_middleware.rs:37` readonly_middleware：CRAP 72.0 > 8（覆盖率 0%）
31. `crates/tui/src/ui.rs:781` draw_trend_full：CRAP 56.0 > 8（覆盖率 0%）
32. `crates/server/src/grpc.rs:141` report_metrics：CRAP 56.0 > 8（覆盖率 0%）
33. `crates/server/src/handlers.rs:217` get_metrics_history：CRAP 56.0 > 8（覆盖率 0%）
34. `crates/tui/src/ui.rs:463` draw_topbar：CRAP 56.0 > 8（覆盖率 0%）
35. `crates/server/src/handlers.rs:631` create_alert_rule：CRAP 42.0 > 8（覆盖率 0%）
36. `crates/server/src/ws_handler.rs:69` broadcast：CRAP 42.0 > 8（覆盖率 0%）
37. `crates/tui/src/ui.rs:73` push：CRAP 42.0 > 8（覆盖率 0%）
38. `crates/server/src/handlers.rs:31` login：CRAP 30.0 > 8（覆盖率 0%）
39. `crates/tui/src/ui.rs:561` draw_nodes：CRAP 30.0 > 8（覆盖率 0%）
40. `crates/agent/src/node_identity.rs:12` load_or_create：CRAP 30.0 > 8（覆盖率 0%）
41. `crates/server/src/grpc.rs:451` update_job_status：CRAP 20.0 > 8（覆盖率 0%）
42. `crates/tui/src/ui.rs:899` mini_chart：CRAP 20.0 > 8（覆盖率 0%）
43. `crates/server/src/handlers.rs:765` create_user：CRAP 20.0 > 8（覆盖率 0%）
44. `crates/server/src/ws_handler.rs:120` ws_upgrade：CRAP 20.0 > 8（覆盖率 0%）
45. `crates/server/src/auth_middleware.rs:11` auth_middleware：CRAP 20.0 > 8（覆盖率 0%）
46. `crates/tui/src/api.rs:302` fmt_bytes：CRAP 20.0 > 8（覆盖率 0%）
… 还有 20 项（完整数据见输出目录的 *.json）

## coverage（31 项）— 为未执行的代码补测试；确属死代码就删除
47. `crates/agent/src/config_loader.rs` 行覆盖率 0.0%（0/27）
48. `crates/agent/src/grpc_client.rs` 行覆盖率 0.0%（0/170）
49. `crates/agent/src/job_executor.rs` 行覆盖率 0.0%（0/208）
50. `crates/agent/src/main.rs` 行覆盖率 0.0%（0/99）
51. `crates/agent/src/node_identity.rs` 行覆盖率 0.0%（0/15）
52. `crates/common/src/lib.rs` 行覆盖率 0.0%（0/6）
53. `crates/protocol/build.rs` 行覆盖率 0.0%（0/14）
54. `crates/protocol/src/lib.rs` 行覆盖率 0.0%（0/10）
55. `crates/server/src/auth_middleware.rs` 行覆盖率 0.0%（0/92）
56. `crates/server/src/main.rs` 行覆盖率 0.0%（0/331）
57. `crates/server/src/ws_handler.rs` 行覆盖率 0.0%（0/135）
58. `crates/storage/src/aggregation.rs` 行覆盖率 0.0%（0/97）
59. `crates/storage/src/alert_queries.rs` 行覆盖率 0.0%（0/230）
60. `crates/storage/src/audit_queries.rs` 行覆盖率 0.0%（0/81）
61. `crates/storage/src/job_queries.rs` 行覆盖率 0.0%（0/265）
62. `crates/storage/src/lib.rs` 行覆盖率 0.0%（0/248）
63. `crates/storage/src/models.rs` 行覆盖率 0.0%（0/122）
64. `crates/storage/src/queries.rs` 行覆盖率 0.0%（0/274）
65. `crates/storage/src/user_queries.rs` 行覆盖率 0.0%（0/158）
66. `crates/tui/src/api.rs` 行覆盖率 0.0%（0/104）
67. `crates/tui/src/main.rs` 行覆盖率 0.0%（0/86）
68. `crates/tui/src/ui.rs` 行覆盖率 4.7%（51/1079）
69. `crates/server/src/handlers.rs` 行覆盖率 7.2%（54/747）
70. `crates/server/src/grpc.rs` 行覆盖率 16.9%（79/468）
71. `crates/common/src/config.rs` 行覆盖率 36.2%（17/47）
… 还有 6 项（完整数据见输出目录的 *.json）
