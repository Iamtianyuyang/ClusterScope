# QA：no-root（普通用户零特权）维度 —— NR1–NR21 + MRG-02

> 本文件是 `qa/constraints.json` 里 `NR-*` 约束块的执行程序，**供第 5 阶段逐条真跑**。
> 起因：用户需求「这个项目是要做一个不用 root 的程序」；上一轮审查只把「无 root」当成一条文档不符（`DOC-21`）记录，
> 没有把它当成一等验收维度。本次增补把它拆成可判定、可复现的条目（见 `GAUNTLET.md`「第 1 阶段增补」）。

## 环境与前置（第 5 阶段不需要本文件的作者在场）

- 机器：`node` = `ssh tianyuyang@172.19.133.164`，Linux x86_64，**用户 `tianyuyang`，uid 3000，无 sudo**（`groups` 只有 `3000`）。
- 仓库：`/public/tianyuyang/code/ClusterScope-review/gh-line`，分支 `gauntlet/audit-gh-line`。
- 产物：`target/release/clusterscope-{agent,server,tui}`（本次取证用 `2026-10-07` 的 release 构建）。
- PostgreSQL 16.4（**从源码编译到 HOME，无 root**）：`/public/tianyuyang/code/ClusterScope-review/pg16`，
  数据目录 `…/pgdata`，连接串 `postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope`。
  `psql`/`pg_ctl` **不在 PATH**，请用完整路径。server 的 config 模板是 `deploy/server.yaml.example`。
- 本机**没有 `docker`，也没有 `docker-compose`**（`podman` 在但零镜像、无外网）——凡涉及 compose 的检查都只能判 FAIL/N-A。
- 一键复跑（脚本是自包含的，源 `qa/harness/env.sh` 的路径全部写死）：

  ```sh
  cd /public/tianyuyang/code/ClusterScope-review/gh-line
  sh qa/harness/no-root-checks.sh              # 全量，约 2 分钟
  sh qa/harness/no-root-checks.sh --no-slow    # 跳过 NR11/NR12 两条带 sleep 的检查
  ```

  每行输出 `<CHECK-ID> PASS|FAIL - detail`，退出码 = FAIL 条数；证据写到 `gauntlet-out/qa/evidence/`。
  **脚本只按自己记录的 PID 停进程**（`qa/README.md` 硬规矩 1）；`pkill -f clusterscope` 会误杀这台共享机器上
  别人正在跑的 agent（本机确实有一个 2026-09-02 启起来的 user 级 agent）。

## 判定口径

| verdict | 含义 |
|---|---|
| `must-hold` | 需求/文档承诺「无 root 可用」，应当 PASS；FAIL = 缺陷 |
| `finding` | **审计预期它不成立**（文档承诺了、实物做不到）；FAIL 就是审查结论 |
| `na` | 本环境或本轮范围不可验证，理由写在该条 |

### 第 5 阶段的配对要求（重要）

`node .gauntlet/gauntlet.mjs gate --profile full` 的 `constraints` 闸门会把 `qa/constraints.json` 里**每一条**
拿去 `qa/qa-report.json` 找 `"constraint": "<id>"` 的检查：没有配对的算 `unproven`、只要有一条非 pass 就算 `violated`，
两种都让闸门 FAIL（`kit/lib/checks.mjs` 的 `constraintsGate`）。本次新增的 **23 条**（`NR-01`…`NR-21`、`NR-06b`、`MRG-02`）
因此需要在 `qa/qa-report.json` 里各加一条**status=pass** 的检查条目——其中：

- `must-hold` / `finding` 两类都必须实测（`finding` 的 pass = 复现出清单预期的那个 FAIL 现象，与既有 29 条 finding 同口径）；
- `NR-17`（`na`）也要有一条 pass 条目，写明「本线无 web/，见 FE-01」；
- `MRG-02` 的条目指向 `qa/merge-plan-requirements.md#M10`（结论由第 6 阶段补全，第 5 阶段只核验夹具存在且 NRM 行齐全）。

## 检查清单

| # | 检查项 | 操作（真实命令，自包含） | 期望结果 | 证实约束 |
|---|---|---|---|---|
| NR1 | 端口不需要特权 | 用 `SO_REUSEADDR` 的 python 探针（`s.setsockopt(...)` 后 `s.bind(('0.0.0.0',8080))`，50051 同理），先 `ss -ltnH \| grep -E ':(8080\|50051)[[:space:]]'` 判断是否空闲；刚停过 server 的端口可能处于 `TIME_WAIT` → 重试 3 次（脚本已实现） | 空闲端口 bind 成功（8080/50051 > 1024，不需要 `CAP_NET_BIND_SERVICE`）；已被别人占用的端口记为「busy 未探测」而不是 FAIL | `NR-01` |
| NR2 | 文档化的 server 启动路径（`README:84-86`） | `cd …/gh-line && sed 's#localhost:5432#127.0.0.1:5432#' deploy/server.yaml.example > /tmp/nr-server.yaml && ./target/release/clusterscope-server /tmp/nr-server.yaml &` 然后 `sleep 6; ss -ltn \| grep -E ':8080\|:50051'; curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/health` | 两个端口都在监听、health=200，**全程没有 root**（`id -u`=3000） | `NR-02` |
| NR3 | agent 路径默认值全部落在 HOME 内 | `./target/release/clusterscope-agent --help`；`rm -rf /tmp/nr-home && mkdir -p /tmp/nr-home && HOME=/tmp/nr-home timeout 6 ./target/release/clusterscope-agent --config-dir /tmp/nr-home/.config/clusterscope; find /tmp/nr-home -maxdepth 4` | `/tmp/nr-home/.config/clusterscope/logs` 被创建（`--config-dir` 同时改 node_id_file 与 log_dir，`crates/agent/src/config_loader.rs:33-40`） | `NR-03` |
| NR4 | `/etc`、`/var/lib`、`/var/log`、`/usr/local/bin` 对本用户不可写 | `for p in /etc/clusterscope /var/lib/clusterscope /var/log/clusterscope-server /usr/local/bin; do mkdir -p $p && echo "CREATED $p" || echo "DENIED $p"; done; stat -c '%U:%G %a' /usr/local/bin` | 四个 DENIED（`/usr/local/bin` 是 `root:root 755`） | `NR-04` |
| NR5 | `systemd --user` 可装可启（专用探针单元，不用 sudo） | `cat > ~/.config/systemd/user/nr-probe-unit.service <<'EOF' … EOF; systemctl --user daemon-reload; systemctl --user enable --now nr-probe-unit.service; systemctl --user is-active nr-probe-unit.service; systemctl --user show nr-probe-unit.service -p FragmentPath --value; systemctl --user disable --now nr-probe-unit.service; rm ~/.config/systemd/user/nr-probe-unit.service` | 创建符号链接、`is-active=active`、`FragmentPath=$HOME/.config/systemd/user/nr-probe-unit.service`，验证后删除 | `NR-05` |
| NR6 | 缺省 `-c /etc/…` 时 agent 不崩 | `env HOME=/tmp/nr-home-noconf timeout 8 ./target/release/clusterscope-agent -c /etc/clusterscope/agent.yaml; find /tmp/nr-home-noconf -maxdepth 4` | 日志出现 `ClusterScope Agent starting`（exit 124 = 被 timeout 收掉），并在 HOME 里建 `~/.local/state/clusterscope-agent`；`--config-dir` 等 5 个覆盖参数都在 `--help` 里（`crates/agent/src/main.rs:13-27`）。**注意**：`/etc/clusterscope/agent.yaml` 不存在时是**静默回退**（`config_loader.rs:9-11`），这是隐患，见 NR7/NR15 | `NR-06` |
| NR7 | 裸启动 server 必须拒绝而不是带着弱密钥起来 | `timeout 15 ./target/release/clusterscope-server; echo exit=$?` | 打印 `Error: refusing to start: jwt_secret is missing/too weak with auth_required: true …` 并退出（exit 1）。README 没写这条前置条件，属隐性要求 | `NR-07` |
| NR8 | CLI/环境变量能绕开所有 root 路径 | `./target/release/clusterscope-agent --help`；`grep -n 'POSTGRES_URL\|JWT_SECRET\|HTTP_ADDR\|GRPC_ADDR\|AUTH_REQUIRED\|AGENT_TOKEN' crates/server/src/main.rs` | agent 有 `--config/--config-dir/--server-addr/--node-id/--agent-token`；server 认 6 个环境变量（`main.rs:174-219`，含无前缀变体） | `NR-08` |
| NR9 | PostgreSQL 的无 root 落地路径 | `/public/tianyuyang/code/ClusterScope-review/pg16/bin/pg_ctl -D /public/tianyuyang/code/ClusterScope-review/pgdata status; echo exit=$?; /public/tianyuyang/code/ClusterScope-review/pg16/bin/psql 'postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope' -tAc 'select version();'; grep -n 'docker compose up' README.md; command -v docker \|\| echo docker-absent` | `pg_ctl status` exit 0、psql 返回 `PostgreSQL 16.4 …`（**uid 3000 自己跑的实例**）；同时确认 README 只给了 `docker compose up` 这条路，而本机 `docker-absent` | `NR-09` |
| NR10 | README 每处「无 root / systemctl --user」逐条对账 | `grep -n '无需 root\|无 root\|root-not\|systemctl --user' README.md`；分类表见 `gauntlet-out/qa/evidence/no-root-readme-claims.txt` | 11 处提及：`:15/:21/:56/:89/:211/:290/:291/:293/:321` 成立；`:87`（docker compose）本环境不成立；`:288/:289`（`systemctl --user … clusterscope-server`）**仓库无对应 unit** → 不成立 | `NR-10` |
| NR11 | 登出后 user 服务是否存活（linger） | `loginctl show-user $USER \| grep '^Linger='`；然后建一个专用单元 `nr-persist-unit.service`（`ExecStart=/bin/sleep 120`、`WantedBy=default.target`）→ `systemctl --user enable --now` → `ssh localhost 'exit 0'` 断开会话 → `sleep 5` → `kill -0 <MainPID>` → 收尾 `disable --now` + 删单元 | 本机 `Linger=yes`，MainPID 存活（进程仍受 user manager 监管）。**这条是每台机器的事实**：linger=no 的节点上，user 服务会随会话结束被杀（见 NR12 的回退质量） | `NR-11` |
| NR12 | `nohup` 回退能把 agent 跑起来，但没有自动重启 | `nohup ./target/release/clusterscope-agent -c /tmp/nr-nohup/agent.yaml >> /tmp/nr-nohup/agent.log 2>&1 & echo pid=$!; sleep 5; kill -0 <pid> && echo alive; grep -c 'ClusterScope Agent starting' /tmp/nr-nohup/agent.log; kill <pid>; sleep 3; kill -0 <pid> \|\| echo gone`；`grep -n 'Restart=always\|nohup' deploy/install-agent.sh` | 起得来、日志有 `starting`、手动 `kill` 后不重启（`Restart=always` 只写在 `systemd --user` 分支里，`install-agent.sh:70,81`）——回退路径没有守护性，这是与 README「节点状态自动切换」承诺的差距 | `NR-12` |
| NR13 | README 的 `systemctl --user … clusterscope-server` 是否有**可复制的**对应 unit | `grep -n 'systemctl --user' README.md`；`find . -name 'clusterscope-server.service' -not -path './target/*' \| wc -l`；`cat ~/.config/systemd/user/clusterscope-server.service`（本机那份是**手写**的：`ExecStart=$HOME/.local/bin/clusterscope-server $HOME/.config/clusterscope/server.yaml`、`WantedBy=default.target`、2026-08-10 建立、当前 disabled/inactive）；`grep -rn 'systemctl --user\|\.config/systemd' docs/ \| wc -l` | README 命中 5 行 `systemctl --user`（288/289/290/291/293，其中 288/289 是 server）；仓库 0 个用户级 server unit；本机那份 unit 不是仓库产物（`ExecStart` 指向 `$HOME/.local/bin`，仓库里没有任何脚本生成它）；docs/ 0 处 → **命令只在这台已经手配过的机器上有效，在干净机器上必然失败** | `NR-13` |
| NR14 | 安装脚本的路径与分支 | `grep -n '\.local/bin\|\.config/clusterscope\|systemctl --user\|nohup' deploy/install-agent.sh`；`bash -n deploy/install-agent.sh` | 全部落在 `~/.local/bin`、`~/.config/clusterscope`、`~/.config/systemd/user`；先试 `systemctl --user`，否则 `nohup`（`:60,79-83`）；脚本可语法检查通过 | `NR-14` |
| NR15 | 随仓库发的两个 unit 是系统级，非 root 装不上 | `grep -n 'User=\|ExecStart=\|WorkingDirectory=\|LogsDirectory=\|WantedBy=' deploy/*.service; id clusterscope; test -d /etc/clusterscope \|\| echo absent; test -w /usr/local/bin \|\| echo read-only` | 两个 unit 都是 `User=clusterscope` + `/usr/local/bin/…` + `/var/lib/clusterscope` + `WantedBy=multi-user.target`；`id clusterscope` 报 no such user；路径都不可写 → 普通用户**装不上**（预期 FAIL=结论） | `NR-15` |
| NR16 | 本机已有「用户级 agent」在生产运行（路径可行的最强证据） | `systemctl --user status clusterscope-agent; ls -l ~/.local/bin/clusterscope-agent ~/.config/clusterscope/; ps -u $(id -un) -o pid,lstart,cmd \| grep clusterscope-agent` | unit 存在且 `active (running)`（本机自 2026-09-02 起），二进制在 `~/.local/bin`，配置在 `~/.config/clusterscope` —— 与 `install-agent.sh` 的路径完全一致 | `NR-16` |
| NR17 | 前端维度的 no-root 复检是否适用 | `ls web 2>&1; find . -maxdepth 3 -name package.json -not -path './target/*' \| wc -l` | 本线无 `web/`、0 个 `package.json` → 记 N/A（前端只在 A/B 两棵树，见 `FE-01`）；若合流后接入 web，须在 M10 里复跑 | `NR-17` |
| NR18 | TUI 在 pty 里普通用户可渲染 | `./target/release/clusterscope-tui -s http://127.0.0.1:8080` 放进 `script -q -c '…' out` 抓 pty（server 先用 NR2 的方式起好） | pty 输出非空且无 panic（本次取证 319 字节、exit 0） | `NR-18` |
| NR19 | HOME 只读时的失败模式 | `mkdir -p /tmp/nr-home-ro && chmod 500 /tmp/nr-home-ro && HOME=/tmp/nr-home-ro timeout 10 ./target/release/clusterscope-agent; echo exit=$?` | `Error: Failed to create log directory: "/tmp/nr-home-ro/.local/state/clusterscope-agent"` + exit 1（`config_loader.rs:39-42` 是硬失败）。**结论**：本程序的「无 root」前提是 **HOME 可写**；HOME 只读的共享节点上 agent 起不来（server 不受影响，它只写 stderr） | `NR-19` |
| NR20 | NVML / sysfs 普通用户可读 | `nvidia-smi -L; cat /sys/class/drm/card0/device/power/runtime_status; cat /sys/block/nvme0n1/device/model` | 三条都成功（本机 6×L20、NVMe 型号可读）→ GPU 监控不需要 root；磁盘健康若要 `smartctl -n standby` 查 SMART 则可能碰权限（`/usr/sbin/smartctl` 存在，本审查不覆盖该路径） | `NR-20` |
| NR21 | 同一个二进制在「零配置文件」下也能起来 | `env POSTGRES_URL='postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope' JWT_SECRET='nr-checks-secret-0123456789abcdef' AUTH_REQUIRED=false ./target/release/clusterscope-server &` → `curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/health`；`lsof -p <pid> \| grep -E '/etc/clusterscope\|/var/(lib\|log)/clusterscope' \| wc -l` | health=200，且没有打开任何 `/etc/clusterscope`、`/var/lib`、`/var/log` 下的文件 → 无 root 部署可以完全不碰系统目录 | `NR-21` |
| MRG-02 | 合流不变量 M10 的夹具齐全 | `grep -n '^| M10 ' qa/merge-plan-requirements.md; grep -c '^| NRM' qa/merge-plan-requirements.md; grep -c '"NR-' qa/constraints.json` | M10 存在、NRM 行 ≥6、`NR-` 约束都在 → 第 6 阶段必须逐条回答（见 `qa/merge-plan-requirements.md` M10） | `MRG-02` |

## 本次已实测到的关键事实（第 5 阶段复跑时应得到同一批结论）

**脚本实跑结果（2026-10-07，node `lyy-node03`，uid 3000）：`PASS=23 FAIL=0`，退出码 0**（`finding` 类的 pass = 复现出清单预期的那个现象）。
证据：`gauntlet-out/qa/evidence/no-root-server.log`、`no-root-readme-claims.txt`。

1. **uid 3000 全流程可行**：server 起在 8080/50051（health=200）、agent 起得来、TUI 在 pty 里渲染（1185 字节、无 panic）、`systemd --user` 可装可启；不需要任何 root 能力，端口也不需要特权（8080/50051 都 bind 成功）。
2. **`Linger=yes`（本机事实，与上一轮交接的「未验证」不同）**：`loginctl show-user tianyuyang` → `Linger=yes`、`State=active`；本机自 2026-09-02 起的 user 级 agent 一直活着。**但这只是这台机器**：linger 是每机配置（`loginctl enable-linger` 要 root），别处可能为 no，故 NR11 必须逐机复跑。
3. **两套部署件矛盾**（对应 `DOC-21`，本次升级为一等验收）：`deploy/*.service` 是系统级（`User=clusterscope`、`/usr/local/bin`、`/var/lib/clusterscope`、`multi-user.target`、还 `After=redis.service`），普通用户装不上；`install-agent.sh` 是用户级（`~/.local/bin`、`~/.config/clusterscope`、`systemd --user` 或 `nohup`）。
4. **README 的无 root 承诺里只有一条在本环境不成立**：`:87` 的 `docker compose up`（本机无 docker/docker-compose、无外网）。其余（`:15/:21/:56/:89/:211/:290-291/:293/:321`）实测成立。**补充证据**：本机 `~/.config/systemd/user/clusterscope-server.service` 是**手写的**用户级 server unit（2026-08-10、当前 disabled/inactive）——说明「用户级跑 server」这条路真的可行，但**仓库和文档都没有给出它**（`NR-13`）。
5. **两条隐性前提没写进 README**：(a) server 不给配置也不用环境变量时**直接拒绝启动**（jwt_secret 守卫，NR7）；(b) agent 要求 **HOME 可写**（NR19），否则起不来。
6. **`deploy/server.service` 的配置路径不存在时是静默回退**：`agent -c /etc/clusterscope/agent.yaml` 在文件缺失时**不报错**，改用 `~/.config` 默认值（`config_loader.rs:9-11`），所以照抄系统级 unit 的人会在不知情的情况下用错配置。
7. **复跑注意（脚本里已处理，人工复跑时也请照做）**：探针要 bind 8080/50051，刚停掉的 server 会让端口短暂处于 `TIME_WAIT` —— 脚本用 `SO_REUSEADDR` + 重试 3 次；`ss` 显示端口已被别人占用时该端口会被记为「busy 未探测」而不是 FAIL。**本机是共享的**：`NR16` 会看到 2 个 `clusterscope-agent` 进程（一个是 2026-09-02 起的老实例，一个是脚本临时起的），脚本只 kill 自己记下的 PID。

## 与既有约束的关系（不重复的部分）

- `OPS-08` 已覆盖「无 root 也能跑 server + agent + TUI」这一条总断言（本文件 NR1/NR2/NR18 是它的可复现拆解，口径更细但不改变其结论）。
- `DOC-21` 已覆盖「README 用 `systemctl --user`、unit 是系统级」这一文档不符；本文件把它升级为 `NR-13`/`NR-14`/`NR-15`/`NR-16` 四条**可判定**的验收条目。
- `OPS-05` 覆盖 compose 不可验证（N/A）；本文件的 `NR-09` 从「无 root 的 PG 落地路径」角度重新提问，结论不同：**存在可行但未文档化的路径**（源码编译到 HOME）。
