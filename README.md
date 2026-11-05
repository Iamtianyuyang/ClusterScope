# ClusterScope

<p align="center">
  <img src="assets/icon.png" width="96" height="96" alt="ClusterScope" />
</p>

<p align="center"><strong>给 Linux GPU 集群的翻牌状态板</strong><br>一个二进制，实时监控 CPU / GPU / 进程 / 趋势；普通用户即可运行，无需 root。</p>

<p align="center">
  <a href="https://github.com/Iamtianyuyang/ClusterScope/releases"><img alt="Release" src="https://img.shields.io/badge/release-v0.1.2-2aa198"></a>
  <a href="https://github.com/Iamtianyuyang/ClusterScope/blob/master/LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey">
  <img alt="Single binary" src="https://img.shields.io/badge/binary-single-3a3d40">
  <img alt="No root" src="https://img.shields.io/badge/root-not%20required-success">
</p>

<p align="center">
  <a href="docs/images/web-overview.png"><img src="docs/images/web-overview.png" alt="ClusterScope 总览：节点、GPU、进程与每核 CPU" width="960" /></a>
</p>

<p align="center"><sub>合成数据演示：5 节点 · 40 GPU；实际界面由 Server 和 Agent 提供真实数据。</sub></p>

**一个二进制，两个角色。** `clusterscope server` 负责聚合、存储与 Web；`clusterscope agent` 在每台节点采集。Web 资产、字体和图标都编译进同一个可执行文件，普通用户即可运行，不需要 root、Docker、Nginx 或前端构建。

| 你得到什么 | 说明 |
|---|---|
| **一眼判断** | 在线节点、GPU 忙碌、平均 CPU、最高温度固定在一屏 |
| **谁在占用** | 每块 GPU 的利用率 / 显存 / 温度，GPU 进程或 Top CPU 进程 |
| **能追多久** | 实时、1 小时、24 小时、7 天趋势；原始 24h，聚合 90 天 |
| **怎么接入** | 命令行脚本，或在 Web 里输入 SSH 用户名 / IP / 密码，Server 自动上传并启动 Agent |

```
Web 浏览器 ── REST / WS :10202 ── clusterscope server ── gRPC :10203 ── clusterscope agent × N
                                        │
                                   PostgreSQL :10204
```

## 目录

- [环境要求](#环境要求) · [快速开始](#快速开始) · [Web](#web) · [添加服务器](#添加服务器) · [配置](#配置)
- [安装方式](#安装方式) · [服务管理](#服务管理) · [数据保留](#数据保留)
- [文档](#文档) · [项目结构](#项目结构) · [测试](#测试) · [常见问题](#常见问题) · [已知限制](#已知限制)

## 环境要求

| 组件 | 要求 |
|------|------|
| 操作系统 | Linux x86_64；GPU 指标需要 NVIDIA 驱动 / NVML |
| 权限 | 普通用户即可，全程无需 root / sudo |
| PostgreSQL | v16+（server 必需；已有实例或装在 HOME，见[快速开始](#快速开始)） |
| 源码编译 | Rust 工具链 + `protoc` |

## 快速开始

需要 Linux x86_64、Rust 工具链与 `protoc`；Server 还需要 PostgreSQL 16+。以下命令全部以普通用户执行。

### 1. 构建

```bash
git clone https://github.com/Iamtianyuyang/ClusterScope.git
cd ClusterScope
cargo build --release
# 产物：target/release/clusterscope（server + agent + Web 都在同一个程序里）

./deploy/package.sh
# 产物：dist/clusterscope-<版本>-<target>.tar.gz，包内只有一个 clusterscope 可执行文件
```

### 2. 启动 Server

```bash
cp deploy/server.yaml.example server.yaml
# 编辑 postgres_url；内网只读可保持 auth_required: false
./target/release/clusterscope server server.yaml
```

已有 PostgreSQL 实例时，把连接串填进 `postgres_url`，先确认能连：

```bash
psql "postgresql://<用户>:<口令>@<主机>:10204/clusterscope" -c 'select version()'
```

<details>
<summary>没有现成的 PostgreSQL？在 HOME 里起一个（无需 root / Docker）</summary>

本集群实测走的就是这条：源码编译到 `~/pg16`，数据目录 `~/pgdata`，用普通账号起停。

```bash
# 1) 源码包解到 HOME 后编译（有外网的机器下好 tar 包传上来，或直接解发行版 tar 包）
./configure --prefix="$HOME/pg16" && make -j"$(nproc)" && make install

# 2) 初始化数据目录（HOME 内，不需要 root）
"$HOME/pg16/bin/initdb" -D "$HOME/pgdata" -U clusterscope --auth=trust

# 3) 起服务（监听 127.0.0.1:10204）并建库
"$HOME/pg16/bin/pg_ctl" -D "$HOME/pgdata" -l "$HOME/pgdata/server.log" -o "-p 10204 -k /tmp" start
"$HOME/pg16/bin/createdb" -h 127.0.0.1 -p 10204 -U clusterscope clusterscope

# 4) 停服务
"$HOME/pg16/bin/pg_ctl" -D "$HOME/pgdata" stop
```

`postgres_url` 填 `postgresql://clusterscope:<口令>@127.0.0.1:10204/clusterscope`；
`initdb` / `pg_ctl` / `psql` 不在 PATH 时用 `$HOME/pg16/bin/` 下的完整路径。

</details>

### 3. 接入节点

```bash
./deploy/install-agent.sh user@host http://SERVER_IP:10203          # node_id 自动取 hostname
./deploy/install-agent.sh user@host http://SERVER_IP:10203 node-02  # 或显式指定 node_id
```

脚本自动：上传二进制 → 写配置（`~/.config/clusterscope/agent.yaml`）→ `systemd --user` 启动（或 nohup）。
10 秒内节点出现在监控中。**node_id 留空时自动使用本机 hostname** —— 天然适配共享 HOME 的集群
（`/public` 等 NFS 挂载场景，一份配置多机共用）。

也可以跳过命令行，在 Web 总览右上角点「添加服务器」：输入目标机的 SSH 用户名、IP 和密码，
Server 会登录目标机、上传同一个 `clusterscope`、写配置并启动 Agent。该操作需要 ClusterScope
管理员账号（应用内角色），SSH 密码只用于本次请求，不落盘、不写日志。

### 4. 打开监控

浏览器打开 `http://SERVER_IP:10202`。Web 资源由 Server 同源托管，无需安装 Node.js、Nginx 或单独前端服务。

Server 配置 `auth_required: false` 时可直接只读监控；开启认证时，在页面右上角登录。

## Web

Server 将 Web 资源直接嵌入二进制并同源托管，默认每 3 秒刷新，同时通过 WebSocket 接收指标变更。无需构建前端，也没有第三方运行时依赖；显示字体是随二进制嵌入的 OFL 资产，打开就是集群状态，没有引导页或宣传区。

### 界面预览

<p align="center">
  <a href="docs/images/web-trend.png"><img src="docs/images/web-trend.png" alt="CPU 与 GPU 实时和历史趋势" width="680" /></a>
  <a href="docs/images/web-mobile.png"><img src="docs/images/web-mobile.png" alt="移动端集群总览" width="240" /></a>
</p>

页面切换使用 160 ms 淡入与轻微位移；只有聚焦行与所选 GPU 的数值会翻牌，所有动效都遵循系统“减少动态效果”设置。

- **总览**：固定列节点与 GPU 状态板（利用率 / 显存 / 温度 / 负载 / 运行时间）、所选节点的每核 CPU 条带、GPU 明细与进程。
- **趋势**：CPU 全宽曲线 + 逐 GPU 曲线；实线为 CPU 占用 / GPU 利用率，虚线为内存 / 显存；支持实时、1 小时、24 小时和 7 天范围（默认 1 小时），历史查询自动合并原始与聚合数据。
- **进程**：选中 GPU 的进程，或选中节点 CPU 占用最高的进程；移动端用分隔列表展示完整命令。

| 操作 | 方式 |
|------|------|
| 选择 GPU | 点击 GPU 单元，或 `j` / `k` |
| 切换节点 | 点击节点名，或 `h` / `l` |
| 切换进程类型 | 点击分段按钮，或 `p` |
| 切换总览 / 趋势 | 点击导航，或 `1` / `2` |
| 立即刷新 | 点击刷新按钮，或 `r` |

界面使用石墨黑钢板、象牙白数据与琥珀选中，logo 保留原来的青色；数据面使用自托管的 Barlow Condensed 紧缩字体，并遵循系统“减少动态效果”设置。触屏与窄屏使用单列布局，所有关键操作都可键盘完成。采不到的数据显示 `—`，不按 0 处理。

扫读顺序是：先看哪台机器忙，再看哪张 GPU / 哪颗核忙，最后看进程和趋势。

### 添加服务器

点总览右上角的「添加服务器」，填写 SSH 用户名、IP / 主机名、密码、端口和可选节点 ID；Server gRPC 地址会自动填入 `10203`。提交后 Server 以**普通用户**登录目标机，把当前运行的 `clusterscope` 上传到 `~/.local/bin`，写 `~/.config/clusterscope/agent.yaml`，再用 `systemd --user`（不可用时 `nohup`）启动 Agent。全程不调用 `sudo`，不写 `/etc` 或 `/usr/local`，也不需要目标机 root 密码。

<p align="center">
  <a href="docs/images/web-add-server.png"><img src="docs/images/web-add-server.png" alt="添加服务器：SSH 用户名、IP、密码、端口和 Server gRPC 地址" width="560" /></a>
</p>

<p align="center"><sub>合成数据演示。目标机使用普通用户，不需要 sudo/root。</sub></p>

- 安装接口要求 **ClusterScope 管理员账号**（应用内角色），未登录时会先弹出登录框；这不是系统管理员，也不提权。
- 安装成功但 Agent 还没上报时，板面上出现一行「待接入」；节点上线后该行自动消失。
- 待接入列表只保存在当前浏览器的 `localStorage`，不保存 SSH 密码；换浏览器或清除站点数据后列表会消失。
- 密码只用于当次安装，不写入 PostgreSQL、不写日志。
- 页面通过普通 HTTP 打开时会提示「只在可信内网使用」；跨公网部署请在前面加 HTTPS 反向代理。

### 颜色语义（石墨黑 / 象牙白 / 琥珀强调）

| 状态 | 规则 |
|------|------|
| 选中 / 主操作 | 琥珀 `#f0b429` |
| 空闲 GPU / CPU 核 | 深灰 `#24272a` |
| 温度 | ≥70°C 琥珀，≥80°C 红色 |
| 显存 | ≥75% 琥珀，≥90% 红色 |
| 离线 | 整卡弱化，状态点标红 |
| 折线 | CPU 象牙白实线、GPU 琥珀实线、显存 / 内存灰色虚线 |

## 数据采集（无 root）

- **NVML**（nvml-wrapper）：设备数、利用率、显存、温度、功耗、compute processes（pid + used VRAM）、per-process SM / memory / encoder / decoder 采样（驱动不支持时为 `—`）
- **`/proc/<pid>/`**：USER（Uid → getpwuid）与 COMMAND（cmdline，识别 `python train.py` 模式）
  - 尽力而为：无权限 / 进程消失 → 显示 `—` / `<restricted>`，**不报错、不退出、不影响 GPU 指标**
- nvidia-smi 仅作 NVML 初始化失败时的回退
- 采集频率与 UI 渲染分离：指标 2s 一次，UI 刷新独立
- CPU 使用率由同一 sysinfo 实例跨周期计算（避免首刷恒 0），swap 等真实入库，采不到报不可用、不伪造 0
- Top CPU 进程由同一 sysinfo 实例每采集周期（约 2s）扫描一次（仅读 CPU / 内存，不解析 cmdline），首个周期为基线不产生数据，权限不足的进程显示 `—` / `<restricted>`

## 配置

### server.yaml 关键项

```yaml
grpc_addr: "0.0.0.0:10203"
http_addr: "0.0.0.0:10202"
postgres_url: "postgresql://user:pass@localhost:10204/clusterscope"
jwt_secret: "change-me-to-a-long-random-string"
agent_token: ""                # 空 = gRPC 不做认证(仅限可信内网);非空则 agent 必须携带相同 token
default_admin_username: "admin"
default_admin_password: "admin123"
auth_required: false           # false = 只读 API 免密码(GET 开放,写操作仍需 JWT)
```

> **默认管理员口令**：代码默认 `default_admin_password` 是 **`admin`**（上表示例值 `admin123` 只是模板，
> 改成它才会生效）。**首次启动后必须立刻修改**：用 `admin` + 该默认口令登录，再调
> `POST /api/users/{id}/password` 换成强口令；对外暴露的实例还要同时设 `auth_required: true`。

### agent.yaml（可多机共享）

```yaml
server_addr: "http://192.168.1.12:10203"
node_id: ""                  # 空 = 自动使用本机 hostname(共享 HOME 集群推荐)
node_id_file: ~/.config/clusterscope/node_id
agent_token: ""              # 与 server 的 agent_token 一致
report_interval_secs: 2
log_dir: ~/.config/clusterscope/logs
collect_process_details: true   # 尽力读取进程 USER/COMMAND,失败自动降级
# 完整模板见 deploy/agent.yaml.example(另有 log_level / disk_mounts 等键)
```

## Agent 认证（agent_token）

gRPC 控制面默认无认证（适合可信内网）。在不可信网络部署时，server 配置 `agent_token`（或环境变量 `AGENT_TOKEN`），
每个 agent 在 `agent.yaml` 里配置相同值（或 `--agent-token`），所有 gRPC 调用都会带上 `Authorization: Bearer <token>`；
token 不匹配的调用被拒绝。生成随机 token：`openssl rand -hex 32`。

## 安装方式

ClusterScope 只有用户级安装：二进制、配置、日志、PostgreSQL 数据目录和 systemd unit 全部在
`~/.local/bin`、`~/.config` 与用户自己的数据目录里。**不需要 root，也不提供需要 sudo 的安装路径。**

| 位置 | 路径 |
|------|------|
| 二进制 | `~/.local/bin/clusterscope` |
| 配置 | `~/.config/clusterscope/{server,agent}.yaml` |
| unit | `~/.config/systemd/user/clusterscope-server.service`、`clusterscope.service` |

所有命令都用当前登录用户执行；不要用 `sudo` 启动 `clusterscope`，也不要把配置或数据目录指向 `/etc`、`/usr/local`。

### 用户级安装（无需 root）

```bash
mkdir -p ~/.local/bin ~/.config/clusterscope ~/.config/systemd/user
cp target/release/clusterscope ~/.local/bin/   # 或 Release 包里的单二进制
cp deploy/server.yaml.example ~/.config/clusterscope/server.yaml   # 改 postgres_url
cp deploy/agent.yaml.example  ~/.config/clusterscope/agent.yaml    # 改 server_addr
cp deploy/clusterscope-server.service deploy/clusterscope.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now clusterscope-server clusterscope
systemctl --user status clusterscope
```

远程节点的 agent 也可以用 `./deploy/install-agent.sh user@host http://SERVER_IP:10203` 装（免密 SSH，同样无需 root）。
它只停自己启动过的那个进程（按 PID 文件 + 核对 `/proc/<pid>/cmdline`），不会碰到同机器上别人的 agent。

<details>
<summary>让 Agent 在登出 / 重启后继续运行（仍不需要 root）</summary>

用户级服务默认随最后一次登录会话结束而停止。若系统策略允许，普通用户可以为自己开启 linger：

```bash
loginctl show-user "$USER" | grep '^Linger='
loginctl enable-linger "$USER"   # 对自己开启；若被策略拒绝，不要提权，改用下面的 crontab
```

被拒绝时，用用户 crontab 在重启后拉起（不需要 root）：

```bash
mkdir -p ~/.config/clusterscope/logs
crontab -e
# 加入一行：
@reboot ~/.local/bin/clusterscope agent -c ~/.config/clusterscope/agent.yaml >> ~/.config/clusterscope/logs/agent.log 2>&1
```

也可以接受 Agent 只在登录期间运行；`nohup` 方式能否跨登出存活取决于系统的
`KillUserProcesses` 策略。linger 和 crontab 都是**每台机器**的配置，换机器要重新确认。
不要为了常驻去申请 root 或使用 `sudo`。

</details>

## 服务管理

```bash
systemctl --user status  clusterscope-server    # 中央服务
systemctl --user restart clusterscope-server
systemctl --user restart clusterscope           # 本机 agent
ssh node-01 'systemctl --user restart clusterscope'

journalctl --user -u clusterscope               # 日志
```

- Server 重启后，Agent 每 60s 自动重新注册，无需人工干预；调度器从 DB 恢复运行中任务，容量不丢失
- 防火墙需放行 server 的 `10203`（Agent gRPC）与 `10202`（Web / REST / WebSocket）
- 节点状态 online / degraded / offline 自动切换
- 同一台机器只应运行一个 agent（重复实例会互相挤掉上报数据）

## 数据保留

| 数据 | 保留 |
|------|------|
| 原始指标（2s 粒度） | 24 小时 |
| 小时级聚合 | 7 天 |
| 天级聚合 | 90 天 |
| 任务日志（job_logs） | 30 天 |
| 节点 / 任务 / 告警 / 审计 | 长期 |

Web 与 REST 的历史查询自动合并三档粒度；超过 24h 的历史只有平均利用率，无逐 GPU 明细。

## 文档

- `docs/api.md` — REST API 完整参考（认证、节点、指标、服务器接入、任务、告警、WebSocket）
- `docs/architecture.md` — 架构与关键设计决策
- `DESIGN.md` — 视觉系统与组件规则（翻牌状态板）
- `PRODUCT.md` — 产品边界与使用场景
- `deploy/*.example` — server / agent 完整配置模板

## 常见问题

- **节点迟迟不出现**：`systemctl --user status clusterscope` 是否运行、`journalctl --user -u clusterscope` 有无报错、server 的 10203 端口是否可达、`agent_token` 是否与 server 一致
- **Web 提示需要登录**：Server 未开 `auth_required: false`，在页面右上角使用账号密码登录
- **GPU 指标缺失**：agent 所在节点需 NVIDIA 驱动（NVML 初始化失败自动回退 nvidia-smi）；纯 CPU 节点仍正常上报 CPU / 内存

## 项目结构

```
crates/
├── clusterscope/ # 唯一的可执行文件：clap 入口，server / agent 两个子命令
├── common/       # 共享类型、配置、告警引擎、任务状态机
├── protocol/     # gRPC protobuf(含 GpuProcess 进程模型)
├── storage/      # PostgreSQL 访问层
├── agent/        # 节点采集器(NVML + /proc + gRPC)，作为库链接进 clusterscope
├── server/       # 中央服务(REST/gRPC/认证/调度/SSH 接入)，作为库链接进 clusterscope
└── scheduler/    # GPU 感知任务调度
web/              # 零依赖 Web 仪表盘（编译时嵌入 clusterscope）
deploy/           # 用户级 systemd unit、安装/打包脚本
assets/           # 项目图标
docs/             # 架构与 API 文档
```

## 测试

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
npm --prefix web test
npm --prefix web run check
```

## 已知限制

- 进程级 SM 采样依赖驱动持续提供样本（本机 NVIDIA GPU 驱动下为 `—`，优雅降级）
- gRPC 未启用 TLS（`tls_enabled` 已预留），生产建议配合内网 / VPN 或自行加 TLS
- 调度器为容量感知的简单 FIFO（无优先级 / 抢占）；`node_id` 作为首选节点，满时自动改派到其它在线节点
- 任务取消通过 SIGTERM 通知进程组，部分进程可能自行忽略信号（可配 force 后升级为 SIGKILL）
- Web 端以只读 CPU / GPU / 进程监控为主；唯一的写操作是管理员通过 SSH 添加服务器
- 历史曲线：原始数据保留 24h，更早范围来自小时级（7 天）与天级（90 天）聚合，聚合行只有平均利用率，无逐 GPU 明细
- `cluster/info` 中 `idle_gpus` / `avg_gpu_utilization` / `active_alerts` 无数据时为 JSON `null`（不会伪造 0）
- 生产建议：PostgreSQL 独立账号、`auth_required: true`、设置 `agent_token` 并定期轮换 JWT secret

## 许可证

Apache-2.0，见 [LICENSE](https://github.com/Iamtianyuyang/ClusterScope/blob/master/LICENSE)。
