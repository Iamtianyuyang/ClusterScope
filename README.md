# ClusterScope

<p align="center">
  <img src="assets/icon.png" width="96" height="96" alt="ClusterScope" />
</p>

<p align="center">
  <a href="docs/images/web-overview.png"><img src="docs/images/web-overview.png" alt="ClusterScope Web 总览：节点、CPU、GPU 与进程" width="1000" /></a>
</p>

**ClusterScope** — 轻量级 Linux 集群监控平台。Web 仪表盘 + 分布式采集，专注 CPU、GPU 与进程，普通用户即可运行，**无需 root**。

<p align="center">
  <a href="https://github.com/Iamtianyuyang/ClusterScope/releases"><img alt="Release" src="https://img.shields.io/badge/release-v0.1.1-2aa198"></a>
  <a href="https://github.com/Iamtianyuyang/ClusterScope/blob/master/LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey">
  <img alt="No root" src="https://img.shields.io/badge/root-not%20required-success">
</p>

- **Agent**:部署在每台 GPU 节点,通过 **NVML** 采集 GPU 利用率/显存/温度/功耗/进程,`/proc` 尽力读取进程用户名与命令行(权限不足自动降级)
- **Server**:聚合所有节点,提供 REST API 与任务调度,持久化到 PostgreSQL
- **Web**：零依赖浏览器仪表盘 —— 多节点总览、每核 CPU、GPU 表格、GPU / CPU 进程面板、实时与历史趋势

```
┌────────────────┐   REST     ┌─────────────────────┐
│  Web 浏览器    │───────────▶│   Central Server     │
└────────────────┘            │  (gRPC + REST)      │
                              └──────────┬──────────┘
                                         │ gRPC :50051
                   ┌─────────────────────┼─────────────────────┐
             ┌─────▼─────┐        ┌──────▼──────┐       ┌──────▼──────┐
             │   Agent   │        │    Agent    │       │    Agent    │
             │ node-01   │        │ node-02     │       │ node-03     │
             └───────────┘        └─────────────┘       └─────────────┘
                   └──────────┬──────────┴──────────┐
                        ┌────▼────┐
                        │PostgreSQL│
                        └─────────┘
```

## 目录

- [快速开始](#quickstart) · [环境要求](#requirements) · [Web](#web) · [数据采集](#collection)
- [配置](#config) · [安装方式](#install) · [服务管理](#services) · [数据保留](#retention)
- [文档](#docs) · [项目结构](#structure) · [测试](#testing) · [常见问题](#faq) · [已知限制](#limitations)

## 环境要求 {#requirements}

| 组件 | 要求 |
|------|------|
| 操作系统 | Linux x86_64(NVIDIA 驱动 + NVML 采集 GPU 指标) |
| 权限 | 普通用户即可,**无需 root** |
| PostgreSQL | v16+(server 必需;无需 root 的两条落地路径见[快速开始](#quickstart)第 2 步;`deploy/docker-compose.yml` 需要本机有 docker) |
| 源码编译 | Rust 工具链 + `protoc` |

## 快速开始 {#quickstart}

### 1. 获取二进制

**方式 A:直接下载 Release 包**

```bash
curl -L -O https://github.com/Iamtianyuyang/ClusterScope/releases/download/v0.1.1/clusterscope-v0.1.1-linux-x86_64.tar.gz
tar xzf clusterscope-v0.1.1-linux-x86_64.tar.gz
# 得到 clusterscope-agent / clusterscope-server（Web 已内嵌）
```

**方式 B:源码编译**(需要 Rust 工具链与 `protoc`,proto 由 `crates/protocol/build.rs` 编译)

```bash
cargo build --release
# 产物:target/release/clusterscope-{agent,server}（Web 已内嵌）
```

### 2. 启动 Server(需要 PostgreSQL)

```bash
cp deploy/server.yaml.example server.yaml
# 编辑:postgres_url 指向你的库;auth_required: false 开启只读免密(推荐内网)
clusterscope-server server.yaml
```

#### 无 root 准备 PostgreSQL(两条可行路径)

**路径 A —— 已经有 PostgreSQL 实例:直接把它填进 `postgres_url`,什么都不用装**

```yaml
postgres_url: "postgresql://<用户>:<口令>@<主机>:5432/clusterscope"
```

```bash
psql "postgresql://<用户>:<口令>@<主机>:5432/clusterscope" -c 'select version()'   # 先确认真的能连
```

**路径 B —— 把 PostgreSQL 装到自己的 HOME(无 root、无 docker、无外网时用这条)**

本集群实测走的就是这条:源码编译到 `~/pg16`,数据目录 `~/pgdata`,用普通账号起停。

```bash
# 1) 源码包解到 HOME 后编译(有外网的机器下好 tar 包传上来,或直接解发行版 tar 包)
./configure --prefix="$HOME/pg16" && make -j"$(nproc)" && make install

# 2) 初始化数据目录(HOME 内,不需要 root)
"$HOME/pg16/bin/initdb" -D "$HOME/pgdata" -U clusterscope --auth=trust

# 3) 起服务(监听 127.0.0.1:5432)并建库
"$HOME/pg16/bin/pg_ctl" -D "$HOME/pgdata" -l "$HOME/pgdata/server.log" -o "-p 5432 -k /tmp" start
"$HOME/pg16/bin/createdb" -h 127.0.0.1 -p 5432 -U clusterscope clusterscope

# 4) 停服务
"$HOME/pg16/bin/pg_ctl" -D "$HOME/pgdata" stop
```

`postgres_url` 填 `postgresql://clusterscope:<口令>@127.0.0.1:5432/clusterscope`;
`initdb` / `pg_ctl` / `psql` 不在 PATH 时用 `$HOME/pg16/bin/` 下的完整路径。

> `docker compose up`(`deploy/docker-compose.yml`,只含 postgres + server)**需要本机有 docker**:
> 本集群实测 `docker` 与 `docker-compose` 都不存在(podman 也没有镜像),因此走的是上面路径 B。

### 3. 部署 Agent(免密 ssh,无需 root)

```bash
./deploy/install-agent.sh user@host http://SERVER_IP:50051          # node_id 自动取 hostname
./deploy/install-agent.sh user@host http://SERVER_IP:50051 node-02  # 或显式指定 node_id
```

脚本自动:上传二进制 → 写配置(`~/.config/clusterscope/agent.yaml`)→ `systemd --user` 启动(或 nohup)。
10 秒内节点出现在监控中。**node_id 留空时自动使用本机 hostname** —— 天然适配共享 HOME 的集群
(`/public` 等 NFS 挂载场景,一份配置多机共用)。

### 4. 打开监控

浏览器打开 `http://SERVER_IP:8080`。Web 资源由 Server 同源托管，无需安装 Node.js、Nginx 或单独前端服务。

Server 配置 `auth_required: false` 时可直接只读监控；开启认证时，在页面右上角登录。

## Web {#web}

Server 将 Web 资源直接嵌入二进制并同源托管，默认每 3 秒刷新，同时通过 WebSocket 接收指标变更。无需构建前端，也没有第三方 Web 依赖。首屏直接呈现集群状态，没有引导页或宣传区。

### 界面预览

<p align="center">
  <a href="docs/images/web-trend.png"><img src="docs/images/web-trend.png" alt="CPU 与 GPU 实时和历史趋势" width="680" /></a>
  <a href="docs/images/web-mobile.png"><img src="docs/images/web-mobile.png" alt="移动端集群总览" width="240" /></a>
</p>

页面切换使用 200 ms 淡入与轻微位移；所有动效都遵循系统“减少动态效果”设置。

- **总览**：节点状态、每块 GPU 的利用率 / 显存 / 温度、每核 CPU 条带，以及 CPU / 内存 / GPU 汇总。
- **趋势**：CPU 与逐 GPU 曲线；支持实时、1 小时、24 小时和 7 天范围，历史查询自动合并原始与聚合数据。
- **进程**：选中 GPU 的进程，或选中节点 CPU 占用最高的进程；趋势页展示该节点全部 GPU 进程。

| 操作 | 方式 |
|------|------|
| 选择 GPU | 点击 GPU 行，或 `j` / `k` |
| 切换节点 | 点击节点，或 `h` / `l` |
| 切换进程类型 | 点击分段按钮，或 `p` |
| 切换总览 / 趋势 | 点击导航，或 `1` / `2` |
| 立即刷新 | 点击刷新按钮，或 `r` |

界面使用与 GitHub 项目图标一致的海军蓝 / 亮青配色，并遵循系统“减少动态效果”设置；触屏与窄屏使用单列布局，所有关键操作都可键盘完成。采不到的数据统一显示 `—`，不会伪造为 0。

### 信息层级

```
第一眼：哪台机器忙         （集群 CPU% + 在线节点 + 忙碌 GPU）
第二眼：哪张 GPU / 哪颗核忙（GPU 进度条 + 每核条带）
第三眼：谁的什么程序       （USER / COMMAND / VRAM）
第四眼：忙了多久           （CPU / GPU / 显存趋势）
```

### 颜色语义（中性色为主，一个青色强调色）

| 状态 | 规则 |
|------|------|
| 选中 / 主操作 | 青色 |
| 空闲 GPU / CPU 核 | 弱化灰色 |
| 温度 | ≥65°C 黄色，≥80°C 红色 |
| 显存 | ≥70% 黄色，≥90% 红色 |
| 离线 | 整卡弱化，状态点标红 |
| 折线 | 利用率青色、显存灰色、CPU 主文字色 |

## 数据采集(无 root) {#collection}

- **NVML**(nvml-wrapper):设备数、利用率、显存、温度、功耗、compute processes(pid + used VRAM)、per-process SM/memory/encoder/decoder 采样(驱动不支持时为 `—`)
- **`/proc/<pid>/`**:USER(Uid → getpwuid)与 COMMAND(cmdline,识别 `python train.py` 模式)
  - 尽力而为:无权限/进程消失 → 显示 `—` / `<restricted>`,**不报错、不退出、不影响 GPU 指标**
- nvidia-smi 仅作 NVML 初始化失败时的回退
- 采集频率与 UI 渲染分离:指标 2s 一次,UI 刷新独立
- CPU 使用率由同一 sysinfo 实例跨周期计算(避免首刷恒 0),swap 等真实入库,采不到报不可用、不伪造 0
- Top CPU 进程由同一 sysinfo 实例每采集周期(约 2s)扫描一次(仅读 CPU/内存,不解析 cmdline),首个周期为基线不产生数据,权限不足的进程显示 `—` / `<restricted>`

## 配置 {#config}

### server.yaml 关键项

```yaml
grpc_addr: "0.0.0.0:50051"
http_addr: "0.0.0.0:8080"
postgres_url: "postgresql://user:pass@localhost:5432/clusterscope"
jwt_secret: "change-me-to-a-long-random-string"
agent_token: ""                # 空 = gRPC 不做认证(仅限可信内网);非空则 agent 必须携带相同 token
default_admin_username: "admin"
default_admin_password: "admin123"
auth_required: false           # false = 只读 API 免密码(GET 开放,写操作仍需 JWT)
```

> **默认管理员口令**:代码默认 `default_admin_password` 是 **`admin`**(上表示例值 `admin123` 只是模板,
> 改成它才会生效)。**首次启动后必须立刻修改**:用 `admin` + 该默认口令登录,再调
> `POST /api/users/{id}/password` 换成强口令;对外暴露的实例还要同时设 `auth_required: true`。

### agent.yaml(可多机共享)

```yaml
server_addr: "http://192.168.1.12:50051"
node_id: ""                  # 空 = 自动使用本机 hostname(共享 HOME 集群推荐)
node_id_file: ~/.config/clusterscope/node_id
agent_token: ""              # 与 server 的 agent_token 一致
report_interval_secs: 2
log_dir: ~/.config/clusterscope/logs
collect_process_details: true   # 尽力读取进程 USER/COMMAND,失败自动降级
# 完整模板见 deploy/agent.yaml.example(另有 log_level / disk_mounts 等键)
```

## Agent 认证(agent_token)

gRPC 控制面默认无认证(适合可信内网)。在不可信网络部署时,server 配置 `agent_token`(或环境变量 `AGENT_TOKEN`),
每个 agent 在 `agent.yaml` 里配置相同值(或 `--agent-token`),所有 gRPC 调用都会带上 `Authorization: Bearer <token>`;
token 不匹配的调用被拒绝。生成随机 token:`openssl rand -hex 32`。

## 安装方式(用户级 / 系统级) {#install}

| 方式 | 需要 root? | 二进制 / 配置 | unit 文件(仓库里现成的) |
|------|-----------|---------------|--------------------------|
| **用户级(默认,推荐)** | **不需要** | `~/.local/bin/` + `~/.config/clusterscope/` | `deploy/clusterscope-server.service`、`deploy/clusterscope-agent.service`(`WantedBy=default.target`,路径全走 `%h`) |
| 系统级(可选) | **需要 root** | `/usr/local/bin/` + `/etc/clusterscope/` | `deploy/server.service`、`deploy/agent.service`(`User=clusterscope`、`WantedBy=multi-user.target`) |

### 用户级安装(无需 root)

```bash
mkdir -p ~/.local/bin ~/.config/clusterscope ~/.config/systemd/user
cp target/release/clusterscope-server target/release/clusterscope-agent ~/.local/bin/   # 或 Release 包里的二进制
cp deploy/server.yaml.example ~/.config/clusterscope/server.yaml   # 改 postgres_url
cp deploy/agent.yaml.example  ~/.config/clusterscope/agent.yaml    # 改 server_addr
cp deploy/clusterscope-server.service deploy/clusterscope-agent.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now clusterscope-server clusterscope-agent
systemctl --user status clusterscope-agent
```

远程节点的 agent 也可以用 `./deploy/install-agent.sh user@host http://SERVER_IP:50051` 装(免密 ssh,同样无需 root)。
它只停自己启动过的那个进程(按 PID 文件 + 核对 `/proc/<pid>/cmdline`),不会碰到同机器上别人的 agent。

**前提:linger(`systemd --user` 的常驻开关)**。用户级服务由 user manager 托管,**默认随最后一次 SSH 登出被杀**;
要它登出后继续跑,必须给这个用户开 linger:

```bash
loginctl show-user "$USER" | grep '^Linger='   # Linger=yes 才会常驻;Linger=no 时登出即死
loginctl enable-linger "$USER"                 # 需要管理员/root 代办(普通用户通常没权限)
```

本机实测 `Linger=yes`,所以用户级 agent 断开 SSH 后仍在跑;`Linger=no` 的机器要先请管理员执行
上面的 `loginctl enable-linger`(否则改用系统级安装)。linger 是**每台机器**的配置,换机器要重新确认。

### 系统级安装(可选,需要 root)

```bash
sudo install -m755 target/release/clusterscope-server target/release/clusterscope-agent /usr/local/bin/
sudo mkdir -p /etc/clusterscope /var/lib/clusterscope
sudo cp deploy/server.yaml.example /etc/clusterscope/server.yaml
sudo cp deploy/agent.yaml.example  /etc/clusterscope/agent.yaml
sudo useradd --system --home /var/lib/clusterscope --shell /usr/sbin/nologin clusterscope   # unit 里的 User=clusterscope
sudo cp deploy/server.service deploy/agent.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now clusterscope-server clusterscope-agent
```

没有 root 就只用用户级那套;两套不要混装(同名服务会互相顶掉)。

## 服务管理 {#services}

```bash
systemctl --user status  clusterscope-server    # 中央服务
systemctl --user restart clusterscope-server
systemctl --user restart clusterscope-agent     # 本机 agent
ssh node-01 'systemctl --user restart clusterscope-agent'

journalctl --user -u clusterscope-agent         # 日志
```

- Server 重启后,Agent 每 60s 自动重新注册,无需人工干预;调度器从 DB 恢复运行中任务,容量不丢失
- 防火墙需放行 server 的 `50051`（Agent gRPC）与 `8080`（Web / REST / WebSocket）
- 节点状态 online / degraded / offline 自动切换
- 同一台机器只应运行一个 agent(重复实例会互相挤掉上报数据)

## 数据保留 {#retention}

| 数据 | 保留 |
|------|------|
| 原始指标(2s 粒度) | 24 小时 |
| 小时级聚合 | 7 天 |
| 天级聚合 | 90 天 |
| 任务日志(job_logs) | 30 天 |
| 节点/任务/告警/审计 | 长期 |

Web 与 REST 的历史查询自动合并三档粒度；超过 24h 的历史只有平均利用率，无逐 GPU 明细。

## 文档 {#docs}

- `docs/api.md` — REST API 完整参考(认证、节点、指标、任务、告警、WebSocket)
- `docs/architecture.md` — 架构与关键设计决策
- `deploy/*.example` — server / agent 完整配置模板

## 常见问题 {#faq}

- **节点迟迟不出现**:`systemctl --user status clusterscope-agent` 是否运行、`journalctl --user -u clusterscope-agent` 有无报错、server 的 50051 端口是否可达、`agent_token` 是否与 server 一致
- **Web 提示需要登录**：Server 未开 `auth_required: false`，在页面右上角使用账号密码登录
- **GPU 指标缺失**:agent 所在节点需 NVIDIA 驱动(NVML 初始化失败自动回退 nvidia-smi);纯 CPU 节点仍正常上报 CPU/内存

## 项目结构 {#structure}

```
crates/
├── common/      # 共享类型、配置、告警引擎、任务状态机
├── protocol/    # gRPC protobuf(含 GpuProcess 进程模型)
├── storage/     # PostgreSQL 访问层
├── agent/       # 节点采集器(NVML + /proc + gRPC)
├── server/      # 中央服务(REST/gRPC/认证/调度)
└── scheduler/   # GPU 感知任务调度
web/             # 零依赖 Web 仪表盘（由 Server 嵌入并托管）
deploy/          # systemd、docker-compose、install-agent.sh
assets/          # 项目图标
docs/            # 架构与 API 文档
```

## 测试 {#testing}

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
npm --prefix web test
npm --prefix web run check
```

## 已知限制 {#limitations}

- 进程级 SM 采样依赖驱动持续提供样本(本机 NVIDIA GPU 驱动下为 `—`,优雅降级)
- gRPC 未启用 TLS(`tls_enabled` 已预留),生产建议配合内网/VPN 或自行加 TLS
- 调度器为容量感知的简单 FIFO(无优先级/抢占);`node_id` 作为首选节点,满时自动改派到其它在线节点
- 任务取消通过 SIGTERM 通知进程组,部分进程可能自行忽略信号(可配 force 后升级为 SIGKILL)
- Web 端仅提供只读 CPU / GPU / 进程监控
- 历史曲线:原始数据保留 24h,更早范围来自小时级(7 天)与天级(90 天)聚合,聚合行只有平均利用率,无逐 GPU 明细
- `cluster/info` 中 `idle_gpus` / `avg_gpu_utilization` / `active_alerts` 无数据时为 JSON `null`(不会伪造 0)
- 生产建议:PostgreSQL 独立账号、`auth_required: true`、设置 `agent_token` 并定期轮换 JWT secret

## 许可证

Apache-2.0,见 [LICENSE](https://github.com/Iamtianyuyang/ClusterScope/blob/master/LICENSE)。
