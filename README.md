# ClusterScope

<p align="center">
  <img src="web/public/icon.svg" width="96" height="96" alt="ClusterScope" />
</p>

```
   ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄
   ████  ░░░░  ░░░░  ░░░░  ░░░░  ░░░░            ████████████████████
   ████  ░░░░  ░░░░  ░░░░  ░░░░  ░░░░            ██   ██   ██   ██   ██
     3 nodes · 18 GPUs · 1 busy                    ██   ██   ██   ██   ██
                                                  ████████████████████
```

**ClusterScope** — 轻量级 Linux GPU 集群监控平台。终端仪表盘 + 分布式采集,普通用户即可运行,**无需 root**。

- **Agent**:部署在每台 GPU 节点,通过 **NVML** 采集 GPU 利用率/显存/温度/功耗/进程,`/proc` 尽力读取进程用户名与命令行(权限不足自动降级)
- **Server**:聚合所有节点,提供 REST API,持久化到 PostgreSQL
- **TUI**:ratatui 终端仪表盘 —— 多节点横向一屏总览、GPU 表格、GPU 进程面板
- **Web**:React 前端(可选)

```
┌────────────────┐   REST/WS   ┌─────────────────────┐
│  TUI / Web     │────────────▶│   Central Server     │
└────────────────┘             │  (gRPC + REST + WS)  │
                               └──────────┬──────────┘
                                          │ gRPC :50051
                    ┌─────────────────────┼─────────────────────┐
              ┌─────▼─────┐        ┌──────▼──────┐       ┌──────▼──────┐
              │   Agent   │        │    Agent    │       │    Agent    │
              │ node-01│        │ node-02  │       │ node-03  │
              └───────────┘        └─────────────┘       └─────────────┘
                    └──────────┬──────────┴──────────┬──────────┘
                         ┌────▼────┐           ┌─────▼─────┐
                         │PostgreSQL│           │  (可选) Redis│
                         └─────────┘           └───────────┘
```

## 快速开始

### 1. 编译

```bash
cargo build --release
```

### 2. 启动 Server(需要 PostgreSQL)

```bash
cp deploy/server.yaml.example server.yaml
# 编辑:postgres_url 指向你的库;auth_required: false 开启只读免密(推荐内网)
clusterscope-server server.yaml
```

无 root 时可编译 PostgreSQL 到用户目录(参考 `docs/`),或 `docker-compose up`(`deploy/`)。

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
 ClusterScope     3 nodes · 18 GPUs · 1 busy · 0 jobs     ! 1 alert     LIVE · 2s
Overview   Jobs   Alerts
──────────────────────────────────────────────────────────────────────────────
┌ node-01 ─────────────┐ ┌ node-02 ─────────────┐ ┌ node-03 ─────────────┐
│● ONLINE                 │ │● ONLINE                 │ │● ONLINE                 │
│ GPU  UTIL      VRAM TEMP│ │ GPU  UTIL      VRAM TEMP│ │ GPU  UTIL      VRAM TEMP│
│  0         0% 10M/45G 31°│ │  0         0% 10M/45G 30°│ │ >0 ██████100% 29.5G/45G 74°│
│  1         0% 10M/45G 31°│ │  1         0% 10M/45G 30°│ │  1         0% 13M/45G 28°│
│  …                       │ │  …                       │ │  …                       │
│ CPU  0%  MEM  6%  GPU 0% │ │ CPU  0%  MEM  6%  GPU 0% │ │ CPU  0%  MEM 14%  GPU17%│
│ 192.168.1.10 · 6 GPU … │ │ 192.168.1.11 · 6 GPU … │ │ 192.168.1.12 · 6 GPU…│
└──────────────────────────┘ └──────────────────────────┘ └────────────────────────┘
──────────────────────────────────────────────────────────────────────────────
PID      USER      SM     VRAM     CPU    COMMAND
2312230  tianyuy…   —      29.5G    0%     python main.py
```

| 快捷键 | 功能 |
|--------|------|
| `j` / `k` | 上/下选择 GPU(进程面板跟随) |
| `h` / `l` | 左/右选择节点 |
| `Tab` / `1` `2` `3` | 切换 Overview / Jobs / Alerts |
| `r` | 手动刷新 |
| `?` | 帮助 |
| `q` | 退出 |

### 信息层级

```
第一眼:哪台机器忙        (TopBar "1 busy" + 面板 GPU 行)
第二眼:哪张 GPU 忙       (黄色进度条 + 高亮 %)
第三眼:谁的什么程序       (Processes 面板:USER / COMMAND / VRAM)
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

## 数据采集(无 root)

- **NVML**(nvml-wrapper):设备数、利用率、显存、温度、功耗、compute processes(pid + used VRAM)、per-process SM/memory/encoder/decoder 采样(驱动不支持时为 `—`)
- **`/proc/<pid>/`**:USER(Uid → getpwuid)与 COMMAND(cmdline,识别 `python train.py` 模式)
  - 尽力而为:无权限/进程消失 → 显示 `—` / `<restricted>`,**不报错、不退出、不影响 GPU 指标**
- nvidia-smi 仅作 NVML 初始化失败时的回退
- 采集频率与 UI 渲染分离:指标 2s 一次,UI 刷新独立

## 配置

### server.yaml 关键项

```yaml
grpc_addr: "0.0.0.0:50051"
http_addr: "0.0.0.0:8080"
postgres_url: "postgresql://user:pass@localhost:5432/clusterscope"
jwt_secret: "change-me-to-a-long-random-string"
default_admin_username: "admin"
default_admin_password: "admin123"
auth_required: false        # false = 只读 API 免密码(GET 开放,写操作仍需 JWT)
```

### agent.yaml(可多机共享)

```yaml
server_addr: "http://192.168.1.12:50051"
node_id: ""                  # 空 = 自动使用本机 hostname(共享 HOME 集群推荐)
node_id_file: ~/.config/clusterscope/node_id
report_interval_secs: 2
log_dir: ~/.config/clusterscope/logs
collect_process_details: true   # 尽力读取进程 USER/COMMAND,失败自动降级
```

## 服务管理

```bash
systemctl --user status  clusterscope-server    # 中央服务
systemctl --user restart clusterscope-server
systemctl --user restart clusterscope-agent     # 本机 agent
ssh node-01 'systemctl --user restart clusterscope-agent'

journalctl --user -u clusterscope-agent         # 日志
```

- Server 重启后,Agent 每 60s 自动重新注册,无需人工干预
- 节点状态 online / degraded / offline 自动切换

## 项目结构

```
crates/
├── common/      # 共享类型、配置、告警引擎、任务状态机
├── protocol/    # gRPC protobuf(含 GpuProcess 进程模型)
├── storage/     # PostgreSQL 访问层
├── agent/       # 节点采集器(NVML + /proc + gRPC)
├── server/      # 中央服务(REST/WS/gRPC/认证)
├── scheduler/   # GPU 感知任务调度
└── tui/         # 终端仪表盘(ratatui)
web/             # React 前端(可选,含图标 web/public/icon.svg)
deploy/          # systemd、docker-compose、nginx、install-agent.sh、tui.sh
docs/            # 架构与 API 文档
```

## 测试

```bash
cargo test --workspace
```

## 已知限制

- 进程级 SM 采样依赖驱动持续提供样本(本机 NVIDIA GPU 驱动下为 `—`,优雅降级)
- Scheduler 未接入 server 主循环(任务提交后停留在 queued)
- 生产建议:gRPC 启用 TLS(`tls_enabled` 已预留)、PostgreSQL 独立账号、`auth_required: true` 并定期轮换 JWT secret
