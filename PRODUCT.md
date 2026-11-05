# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Existing codebase: dependency-free static HTML/CSS/ES modules under `web/`, embedded into the server binary with `include_str!` and served under a strict same-origin CSP. No framework, no build step, no external fonts or CDNs.

## Users

实验室与小团队的集群使用者、维护者在办公桌浏览器上日常查看；同样包括在 GPU 节点上提交训练/推理任务、想确认卡是否空闲的同学。主要不是专业的 24/7 NOC 值班人员。

## Product Purpose

把一组 Linux GPU 节点的 CPU、GPU、显存、温度、功耗与进程集中到一个页面上，让人在几秒内判断集群是否健康、哪块 GPU 空闲、卡被谁占用、哪个节点掉线。成功意味着少一次 SSH 逐台排查。

## Positioning

单二进制、普通用户权限即可运行的轻量监控：Agent 用 NVML 采集 GPU、用 `/proc` 尽力读取进程，Web 资产内嵌在 Server 中，浏览器端零依赖。

## Operating Context

Linux x86_64 + NVIDIA 驱动；Server 需要 PostgreSQL，Agent 无需 root；部署在内网，浏览器与 Server 同源；默认每 3 秒刷新，WebSocket 可用时走实时推送；历史趋势按 1 小时 / 24 小时 / 7 天查看。用户通常同时开着终端或其他工具，只会周期性切回这个页面扫一眼。

## Capabilities and Constraints

- 总览：多节点状态、每节点 GPU 列表（利用率、显存、温度）、每核 CPU、CPU/内存/负载摘要。
- 进程：选中某块 GPU 后查看该卡的 GPU 进程，或切换为 Top CPU 进程。
- 趋势：选中节点的 CPU 与各 GPU 利用率/显存曲线，支持实时、1 小时、24 小时、7 天。
- 登录：Server 开启认证时用用户名/密码登录，token 刷新与退出。
- 服务器接入：管理员在 Web 里填写 SSH 用户名 / IP / 密码，Server 把同一个 `clusterscope` 上传到目标用户的 HOME 并启动 Agent；目标机无需 root，密码不落盘。
- 约束：Web 资产编译期内嵌，CSP 仅允许同源资源；不得引入外部字体、CDN、前端框架或构建步骤；REST/WS 契约不变。
- 事实未定：是否新增告警/任务页面不在本次范围，现有 API 虽提供 jobs/alerts，但当前 Web 没有对应界面。

## Brand Commitments

产品名 **ClusterScope** 与 `assets/icon.png` 必须保留。用户明确表示只保留 logo 与功能，配色、主题、布局与组件语言均可重新设计；logo 中的青色不作为全局配色约束。

## Evidence on Hand

- `docs/images/web-overview.png`、`web-trend.png`、`web-add-server.png`、`web-mobile.png`：当前界面截图，使用 5 节点 / 40 GPU 合成数据，用于 README 与视觉回归；不是生产数据。
- `README.md`：功能、部署方式与权限约束的事实来源。
- `docs/api.md`：REST/WS 契约。
- 没有真实生产数据样本；演示与截图使用合成数据时必须标注。

## Product Principles

1. **一眼可判**：先回答"集群好不好、卡闲不闲、谁在占"，再提供细节。
2. **数据优先于装饰**：颜色、动效与图形只在承载状态、层级或变化时出现。
3. **低摩擦**：打开即读，不需要配置、不需要 root、不需要懂内部结构。
4. **异常才打扰**：离线、过热、显存吃紧要显眼；正常状态保持安静。
5. **扫读友好**：为周期性回看设计，位置稳定、数字对齐、状态可比较。

## Accessibility & Inclusion

键盘可完成页面切换、节点/GPU 选择与刷新；焦点可见；尊重 `prefers-reduced-motion`；正文与控件满足 WCAG AA 对比度；不能只靠颜色区分状态。
