# 无 root 部署下 agent 的配置加载与身份文件（本轮修复 NF-01）
#
# 场景名 ↔ Rust 测试名的硬契约（commands 适配器；`.gauntlet/lib/adapter-commands.mjs:130-163` 的 matchAcceptance）：
#   闸门把「场景名」和「每个测试的 JUnit 名」都转小写、折叠空白后做**子串**匹配。
#   Rust 标识符里不能有空格，所以这里的场景名整体写成 snake_case，编码阶段照这个名字写测试函数：
#     场景: agent_refuses_to_start_when_the_explicit_config_file_is_missing
#     ⇔ Rust: fn agent_refuses_to_start_when_the_explicit_config_file_is_missing()
#        （libtest 名 config_loader::tests::agent_refuses_to_start_when_… 逐字包含该场景名）
#   反向要求：**不要**把场景名改写成带空格或中文的散文——那样 ACCEPTANCE 闸门永远找不到对应测试。
#
# 覆盖的约束（qa/constraints.json）：FIX-02 / FIX-03 / FIX-04 / FIX-05。
# 驱动方式（编码阶段二选一，都合法）：场景 1–4、6 断言的是**进程**行为（退出码 + 输出），
#   建议用集成测试 spawn 真二进制（`env!("CARGO_BIN_EXE_clusterscope-agent")`）；场景 5 可在进程内断言目录与文件。
# 别名契约：本文件不引入任何新 CLI 参数——`clusterscope-agent` 现有的 `-c/--config`、`--config-dir` 就是边界。
#
# 前置事实（2026-10-07 实测，证据 qa/evidence/no-root-verify*.txt、no-root-verify2-a-*.txt、no-root-verify3-q1-*.txt）：
#   1) 显式 `-c /path/missing.yaml` 时现状 exit 1，但错误只说 `Failed to write node identity`，
#      **从不告诉用户配置文件不存在**（输出里点名该文件的次数 = 0）；
#   2) 同一次运行里已经按默认 `http://localhost:50051` 起步（静默回退；对照实验：配置写 59999 → 实际拨 59999）；
#   3) 干净 HOME（没有 `~/.config`）下写 `~/.config/node_id` 失败并 exit 1 —— 没有创建父目录。

功能: 无 root 部署下 agent 的配置加载与身份文件

  背景:
    假如 一个干净、可写的 HOME 目录，里面没有任何 clusterscope 配置文件

  场景: agent_refuses_to_start_when_the_explicit_config_file_is_missing
    假如 一个位于临时目录、并不存在的配置文件路径（连它的父目录都不存在）
    当 我以 "-c" 显式指定该路径启动 clusterscope-agent
    那么 进程以非 0 退出码结束
    而且 输出中逐字出现那个不存在的配置路径
    而且 输出说明该配置文件不存在（"not found" / "No such file" / "does not exist" 等同义措辞）
      """
      $ target/release/clusterscope-agent -c /tmp/absent-dir/agent.yaml ; echo exit=$?
      exit=1                     # 现状已经是 1，但输出里从不点名 /tmp/absent-dir/agent.yaml
      """

  场景: missing_config_error_points_at_the_user_level_config_path
    假如 一个并不存在的配置文件路径
    当 我以 "-c" 显式指定该路径启动 clusterscope-agent
    那么 输出中提到用户级安装的配置位置 ".config/clusterscope/agent.yaml"
    # 目的：照抄系统级 unit（`ExecStart=… --config /etc/clusterscope/agent.yaml`）的人一眼就知道
    #       要换成 install-agent.sh 写出来的那条用户级路径，而不是继续对着不存在的文件猜。

  场景: missing_config_is_not_silently_replaced_by_the_default_server_address
    假如 一个并不存在的配置文件路径
    当 我以 "-c" 显式指定该路径启动 clusterscope-agent
    那么 输出中不出现默认服务器地址 "http://localhost:50051"
    而且 进程没有声称自己已经按默认配置启动
    # 反例：修「报错不清楚」的时候不能退化成「照旧静默用默认值」。

  场景: agent_warns_about_the_missing_default_config_file_and_keeps_running
    假如 没有传 "-c"，而默认配置路径 "/etc/clusterscope/agent.yaml" 在这台机器上不存在
    当 我启动 clusterscope-agent
    那么 输出中点名默认配置路径 "/etc/clusterscope/agent.yaml"，说明它没找到、改用内置默认值
    而且 进程仍然在运行（没有被硬错误终止）
    # 这一条守住既有的 NR-06（must-hold：不带 -c 时 agent 不得崩）；
    # 只是回退不再**静默**——必须在输出里说出来。

  场景: agent_creates_the_missing_parent_directory_of_the_node_identity_file
    假如 一个干净 HOME，其中 ".config" 目录不存在
    而且 一份有效的配置文件，未显式指定 node_id_file（即用默认 "$HOME/.config/node_id"）
    当 我以 "-c" 指定该配置文件启动 clusterscope-agent
    那么 HOME 下的 ".config/node_id" 被创建且内容非空
    而且 输出中没有 "Failed to write node identity"
    而且 进程在 5 秒后仍然在运行

  场景: configured_server_address_is_the_address_the_agent_uses
    假如 一份有效的配置文件，其 server_addr 写的是 "http://127.0.0.1:59999"
    当 我以 "-c" 指定该配置文件启动 clusterscope-agent
    那么 输出中的启动横幅显示 "http://127.0.0.1:59999"
    而且 输出中不出现默认地址 "http://localhost:50051"
    # 正例对照：防止「把报错修好」的同时把配置文件整个忽略掉
    # （审查阶段的对照实验就是这一条：配置写 59999，实际拨的必须是 59999）。
