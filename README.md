# ClusterScope

<p align="center">
  <img src="assets/icon.png" width="96" height="96" alt="ClusterScope" />
</p>

```
   ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄
   ████  ░░░░  ░░░░  ░░░░  ░░░░  ░░░░            ████████████████████
   ████  ░░░░  ░░░░  ░░░░  ░░░░  ░░░░            ██   ██   ██   ██   ██
     3 nodes · 18 GPUs · 1 busy                    ██   ██   ██   ██   ██
                                                  ████████████████████
```

**ClusterScope** — 轻量级 Linux GPU 集群监控平台。终端仪表盘(TUI)+ 分布式采集,普通用户即可运行,**无需 root**。

<p align="center">
  <a href="https://github.com/Iamtianyuyang/ClusterScope/releases"><img alt="Release" src="https://img.shields.io/badge/release-v0.1.1-2aa198"></a>
  <a href="https://github.com/Iamtianyuyang/ClusterScope/blob/master/LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey">
  <img alt="No root" src="https://img.shields.io/badge/root-not%20required-success">
</p>

- **Agent**:部署在每台 GPU 节点,通过 **NVML** 采集 GPU 利用率/显存/温度/功耗/进程,`/proc` 尽力读取进程用户名与命令行(权限不足自动降级)
- **Server**:聚合所有节点,提供 REST API 与任务调度,持久化到 PostgreSQL
- **TUI**:ratatui 终端仪表盘 —— 多节点横向一屏总览、GPU 表格、GPU 进程面板、实时折线图(Trend)

```
┌────────────────┐   REST     ┌─────────────────────┐
│  TUI           │───────────▶│   Central Server     │
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

## 快速开始

### 1. 获取二进制

**方式 A:直接下载 Release 包**

```bash
curl -L -O https://github.com/Iamtianyuyang/ClusterScope/releases/download/v0.1.1/clusterscope-v0.1.1-linux-x86_64.tar.gz
tar xzf clusterscope-v0.1.1-linux-x86_64.tar.gz
# 得到 clusterscope-agent / clusterscope-server / clusterscope-tui
```

**方式 B:源码编译**(需要 protoc,见 `docs/`)

```bash
cargo build --release
```

### 2. 启动 Server(需要 PostgreSQL)

```bash
cp deploy/server.yaml.example server.yaml
# 编辑:postgres_url 指向你的库;auth_required: false 开启只读免密(推荐内网)
clusterscope-server server.yaml
```

无 root 时可用 `docker compose up`(`deploy/docker-compose.yml`,只含 postgres + server)。

### 3. 部署 Agent(免密 ssh,无需 root)

```bash
./deploy/install-agent.sh user@host http://SERVER_IP:50051
```

脚本自动:上传二进制 → 写配置(`~/.config/clusterscope/agent.yaml`)→ `systemd --user` 启动(或 nohup)。
10 秒内节点出现在监控中。**node_id 留空时自动使用本机 hostname** —— 天然适配共享 HOME 的集群
(`/public` 等 NFS 挂载场景,一份配置多机共用)。

### 4. 打开监控

```bash
clusterscope-tui                      # 免密码(server 配 auth_required: false)
./deploy/tui.sh                       # 一键脚本(tmux 会话,可重连)
```

SSH 登录自动打开(加入 `~/.bashrc`):

```bash
if [ -z "$TMUX" ] && [ -n "$PS1" ]; then clusterscope-tui; fi
```

## TUI

```
 ClusterScope     3 nodes · 18 GPUs · 13 busy · 0 jobs · CPU 12%     ! 1 alert     LIVE · 1s
Overview   Jobs   Alerts   Trend
──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
┌ node-01 ─────────────────────────────────────────────────┐┌ node-02 ─────────────────────────────────────────────────┐┌ node-03 ─────────────────────────────────────────────────┐
│● ONLINE                                                     ││● ONLINE                                                     ││● ONLINE                                                     │
│ GPU  UTIL      VRAM   TEMP                                  ││ GPU  UTIL      VRAM   TEMP                                  ││ GPU  UTIL      VRAM   TEMP                                  │
│  0  ██████ 95% 10.0G/45G 42°                                ││  0  ██████ 98% 10.0G/45G 42°                                ││ >0  ██████100% 37.5G/45G 76°                                │
│  1  ██████ 98% 10.0G/45G 41°                                ││  1  ██████ 98% 10.0G/45G 41°                                ││  1          0% 13M/45G 29°                                  │
│  2  ██████ 98% 10.0G/45G 40°                                ││  2          0% 13M/45G 28°                                  ││  2  ██████ 98% 10.0G/45G 39°                                │
│  3  ██████ 98% 10.0G/45G 41°                                ││  3          0% 13M/45G 28°                                  ││  3  ██████ 96% 10.0G/45G 39°                                │
│  4  ██████ 98% 10.0G/45G 41°                                ││  4          0% 13M/45G 30°                                  ││  4  ██████ 98% 10.0G/45G 40°                                │
│  5  ██████ 98% 10.0G/45G 38°                                ││  5          0% 13M/45G 28°                                  ││  5  ██████ 98% 10.0G/45G 37°                                │
│ cores ██░░███░████░░░░███░                                  ││ cores ░░░░░░░█░░░░░░░░░░░                                  ││ cores ████░░██░░░█████░░░░                                  │
│ CPU  0%     MEM 17% █   GPU 98% ███                         ││ CPU  0%     MEM 10%     GPU 33% █                           ││ CPU  0%     MEM 22% █   GPU 82% ██                          │
│ 192.168.1.10 · 6 GPU · load 2.4                           ││ 192.168.1.11 · 6 GPU · load 1.0                           ││ 192.168.1.12 · 6 GPU · load 3.8                           │
│                                                             ││                                                             ││                                                             │
│                                                             ││                                                             ││                                                             │
│                                                             ││                                                             ││                                                             │
│                                                             ││                                                             ││                                                             │
│                                                             ││                                                             ││                                                             │
└─────────────────────────────────────────────────────────────┘└─────────────────────────────────────────────────────────────┘└─────────────────────────────────────────────────────────────┘
──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
```

| 快捷键 | 功能 |
|--------|------|
| `j` / `k` | 上/下选择 GPU(进程面板跟随) |
| `p` | 进程面板切换:GPU 进程 ↔ 节点 Top CPU 进程 |
| `h` / `l` | 左/右选择节点(Trend 页切换曲线节点) |
| `Tab` / `1` `2` `3` `4` | 切换 Overview / Jobs / Alerts / Trend |
| `r` | 手动刷新 |
| `?` | 帮助 |
| `q` | 退出 |

Overview 页保留原始布局(每节点全部 GPU 的 UTIL/VRAM/TEMP 表格);面板高度不足时自动压缩为每行 2/3/6 张卡,保证**全部显卡始终可见**。GPU 表下方是**每核 CPU 使用率条带**(每核一格,空闲 `░`、忙碌 `█`、≥90% 黄),单核跑满一眼可见;agent 未上报核级数据时该行不显示。顶栏显示**集群平均 CPU%**(无数据时不显示,不伪造 0)。

Trend 页每张 GPU 一张独立小图,图内两条线:**性能占用(黄)+ 显存占用(青)**,另有 CPU 图(浅蓝);`h`/`l` 切换节点,下方进程面板显示该节点**所有 GPU 上全部用户的进程**(带 GPU 列)。CPU 图上方一行**每核实时占用条带**,与 Overview 面板同款图例。

按 `p` 把进程面板切成 **Top CPU 进程**视图:显示选中节点 CPU 占用最高的 15 个进程(PID / USER / CPU% / MEM / COMMAND),agent 每采集周期(约 2s)扫描一次,扫描开销约 20ms/次(4000 进程节点实测),不拖慢计算任务;采样窗口内采不到数据时显示提示而非伪造 0。

```
 Trend: node-01   U ── util   M ── mem   · 60 samples · 3s · h/l node
 cores ████████ ████████ ██████░░░…
┌ CPU    0%──────┐┌ GPU0  U 98% M 22%──┐┌ GPU1  U  0% M  0%──┐┌ GPU2  U  0% M  0%──┐
│⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀││⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉││⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤││⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤│
└────────────────┘└────────────────────┘└────────────────────┘└────────────────────┘
┌ GPU3  U 89% M 38%────┐┌ GPU4  U  0% M 22%────┐┌ GPU5  U 90% M 24%────┐
│⢀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀││⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉││⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀│
└───────────────────────┘└───────────────────────┘└───────────────────────┘
 Processes      node-01 / GPU all GPUs
──────────────────────────────────────────────────────────────────────────────
GPU  PID      USER      SM     VRAM     CPU    COMMAND
  3  1598696  xuepeng…    91%    16.9G     0%  python train_seismic_titok_proxy_stage1_ddp.py
  4  1598824  xuepeng…    88%    10.9G     0%  python train_seismic_titok_proxy_stage2_ddp.py
```

### 信息层级

```
第一眼:哪台机器忙        (TopBar 集群 CPU% + "N busy" + 面板 GPU 行)
第二眼:哪张 GPU 忙 / 哪颗核热 (GPU 进度条 + 每核条带黄格)
第三眼:谁的什么程序       (Processes 面板:USER / COMMAND / VRAM)
第四眼:忙了多久           (Trend 页折线:利用率 / 显存占用曲线)
```

### 颜色语义(90% 中性,10% 表达状态)

| 状态 | 颜色 | 规则 |
|------|------|------|
| 空闲 GPU | 弱化 dim | 无进度条底纹,`0%` 淡显 |
| 忙碌 GPU | 白色加粗 / 黄色 | ≥1% 显示 `██████` 条;95%+ 黄色(繁忙≠故障) |
| 温度 | 白 → 黄 → 红 | ≥65°C 黄,≥80°C 红(只作用于温度字段) |
| 显存 | 白 → 黄 → 红 | ≥70% 黄,≥90% 红 |
| 告警 | 黄色加粗 | TopBar `! N alert` |
| 选中 | teal | 面板边框 + `>` 标记,无大面积背景 |
| 离线 | ○ + dim | 整卡弱化,不刷红 |
| 折线·性能占用 | 黄 | Trend 页每张 GPU 图的利用率线 |
| 折线·显存占用 | 青 | Trend 页每张 GPU 图的内存占用线 |
| 折线·CPU | 浅蓝 | Trend 页 CPU 曲线 |
| 每核条带·空闲核 | dim `░` | Overview/Trend 每核使用率条带 |
| 每核条带·忙碌核 | 白 `█` | ≥1% 占用 |
| 每核条带·热核 | 黄 `█` | ≥90% 单核跑满 |

## 数据采集(无 root)

- **NVML**(nvml-wrapper):设备数、利用率、显存、温度、功耗、compute processes(pid + used VRAM)、per-process SM/memory/encoder/decoder 采样(驱动不支持时为 `—`)
- **`/proc/<pid>/`**:USER(Uid → getpwuid)与 COMMAND(cmdline,识别 `python train.py` 模式)
  - 尽力而为:无权限/进程消失 → 显示 `—` / `<restricted>`,**不报错、不退出、不影响 GPU 指标**
- nvidia-smi 仅作 NVML 初始化失败时的回退
- 采集频率与 UI 渲染分离:指标 2s 一次,UI 刷新独立
- CPU 使用率由同一 sysinfo 实例跨周期计算(避免首刷恒 0),swap 等真实入库,采不到报不可用、不伪造 0
- Top CPU 进程由同一 sysinfo 实例每采集周期(约 2s)扫描一次(仅读 CPU/内存,不解析 cmdline),首个周期为基线不产生数据,权限不足的进程显示 `—` / `<restricted>`

## 配置

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

### agent.yaml(可多机共享)

```yaml
server_addr: "http://192.168.1.12:50051"
node_id: ""                  # 空 = 自动使用本机 hostname(共享 HOME 集群推荐)
node_id_file: ~/.config/clusterscope/node_id
agent_token: ""              # 与 server 的 agent_token 一致
report_interval_secs: 2
log_dir: ~/.config/clusterscope/logs
collect_process_details: true   # 尽力读取进程 USER/COMMAND,失败自动降级
```

## Agent 认证(agent_token)

gRPC 控制面默认无认证(适合可信内网)。在不可信网络部署时,server 配置 `agent_token`(或环境变量 `AGENT_TOKEN`),
每个 agent 在 `agent.yaml` 里配置相同值(或 `--agent-token`),所有 gRPC 调用都会带上 `Authorization: Bearer <token>`;
token 不匹配的调用被拒绝。生成随机 token:`openssl rand -hex 32`。

## 任务与告警(通过 REST API 管理)

TUI 以只读方式展示任务与告警;提交任务、停任务、管理告警规则使用 REST API(需要 JWT):

```bash
# 登录拿 token(auth_required: false 时只读接口可免 token)
TOKEN=$(curl -s -X POST http://SERVER:8080/api/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"admin123"}' | jq -r .access_token)

# 提交任务(调度器按 GPU 容量派发到 online 节点;node_id 为首选节点,满时自动改派)
curl -X POST http://SERVER:8080/api/jobs -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"node_id":"node-01","name":"train","executable":"/usr/bin/python3",
       "arguments":["train.py","--epochs", "10"],"working_directory":"/home/user/work",
       "environment":{"CUDA_VISIBLE_DEVICES":"0"}}'

# 停任务(agent 收到 stopping 后 SIGTERM 进程组)
curl -X DELETE http://SERVER:8080/api/jobs/<job_id> -H "Authorization: Bearer $TOKEN"

# 建告警规则(metric: gpu_temperature / gpu_utilization / gpu_memory_used_percent /
#            gpu_power_watts / cpu_usage_percent / memory_usage_percent / load_1)
curl -X POST http://SERVER:8080/api/alerts/rules -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"高温","metric":"gpu_temperature","operator":"gte","threshold":85,
       "duration_seconds":60,"severity":"critical"}'
```

完整接口见 `docs/api.md`。

## 服务管理

```bash
systemctl --user status  clusterscope-server    # 中央服务
systemctl --user restart clusterscope-server
systemctl --user restart clusterscope-agent     # 本机 agent
ssh node-01 'systemctl --user restart clusterscope-agent'

journalctl --user -u clusterscope-agent         # 日志
```

- Server 重启后,Agent 每 60s 自动重新注册,无需人工干预;调度器从 DB 恢复运行中任务,容量不丢失
- 节点状态 online / degraded / offline 自动切换
- 同一台机器只应运行一个 agent(重复实例会互相挤掉上报数据)

## 数据保留

| 数据 | 保留 |
|------|------|
| 原始指标(2s 粒度) | 24 小时 |
| 小时级聚合 | 7 天 |
| 天级聚合 | 90 天 |
| 任务日志(job_logs) | 30 天 |
| 节点/任务/告警/审计 | 长期 |

TUI 与 REST 的历史查询自动合并三档粒度;超过 24h 的历史只有平均利用率,无逐 GPU 明细。

## 项目结构

```
crates/
├── common/      # 共享类型、配置、告警引擎、任务状态机
├── protocol/    # gRPC protobuf(含 GpuProcess 进程模型)
├── storage/     # PostgreSQL 访问层
├── agent/       # 节点采集器(NVML + /proc + gRPC)
├── server/      # 中央服务(REST/gRPC/认证/调度)
├── scheduler/   # GPU 感知任务调度
└── tui/         # 终端仪表盘(ratatui)
deploy/          # systemd、docker-compose、install-agent.sh、tui.sh
assets/          # 项目图标
docs/            # 架构与 API 文档
```

## 测试

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## 已知限制

- 进程级 SM 采样依赖驱动持续提供样本(本机 NVIDIA GPU 驱动下为 `—`,优雅降级)
- gRPC 未启用 TLS(`tls_enabled` 已预留),生产建议配合内网/VPN 或自行加 TLS
- 调度器为容量感知的简单 FIFO(无优先级/抢占);`node_id` 作为首选节点,满时自动改派到其它在线节点
- 任务取消通过 SIGTERM 通知进程组,部分进程可能自行忽略信号(可配 force 后升级为 SIGKILL)
- 当前只保留 TUI 终端仪表盘(Web 前端已移除);任务/告警管理通过 REST API 完成
- 历史曲线:原始数据保留 24h,更早范围来自小时级(7 天)与天级(90 天)聚合,聚合行只有平均利用率,无逐 GPU 明细
- `cluster/info` 中 `idle_gpus` / `avg_gpu_utilization` / `active_alerts` 无数据时为 JSON `null`(不会伪造 0)
- 生产建议:PostgreSQL 独立账号、`auth_required: true`、设置 `agent_token` 并定期轮换 JWT secret
