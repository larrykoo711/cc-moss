# 004: feature — Stage 3 Moss agent

## 来源
- 无 tracker，序号 004（见 `.sdd/config.md` `{EDITION}`）；原规划的 004-pet-approve 并入本档案（R3），005-voice 顺延
- 触发：v0.2.2 发布后，用户 2026-10-18 提出下一个大 Stage（R1–R7）

## 需求摘要
- 七条需求与证据见 `notes/drivers-evidence.md`：
  - R1 新会话默认 bypass（App 与 CLI）
  - R2 内置 Moss 会话 + Doctor（预制 prompt，一键检测 / 整理 / 修复 / 规范会话）
  - R3 可选：会话等用户答题时，交给 Moss 用自己的选择框 / 输入框代答（hook 内置，留给远程 IM）
  - R4 Moss 常驻守护会话 + 系统级守护进程（参考项目移植）
  - R5 Moss agent 只做调度：派发 bypass 会话、判断目录、拿不准问用户
  - R6 首个外部渠道 Slack App，自定义人设
  - R7 交互路由：桌面宠物 / 外部渠道，在场检测后自动切换
- 性质：能力建设 epic，架构级（`arch_level: yes`）
- 边界：只做 macOS；公开仓不写参考项目的仓库名

## 目录索引
| 文件 | 用途 |
|------|------|
| `notes/drivers-evidence.md` ⭐ | 立项依据与证据 |
| `notes/current-state-baseline.md` | S1：现有能力现状（✅/⚠️/❌ + file:line） |
| `notes/reference-study.md` | S1：参考项目的模块学习与移植评估 |
| `notes/claude-code-facts.md` | S1：Claude Code 外部事实（bypass、代答 hook、无头模式） |
| `notes/slack-presence-facts.md` | S1：Slack App 与 macOS 在场检测的外部事实 |
| `DESIGN-moss-agent.md` ⭐ | S1：权威设计（假设、capability map、架构、开放问题） |
| `notes/anchors.md` | S1：锚点核实（2026-10-18） |
| `diagrams/architecture-moss-agent.html` | 架构图 |
| `diagrams/sequence-ask.html` | 代答时序图 |
| `diagrams/src/` | 两张图的 archify 源 JSON |
| `PLAN.md` ⭐ | 4 Phase × 11 Task + R1、依赖图、Kanban |
| `playground.html` | 可视化档案（背景、缺口、方案、锚点、进展、反馈车道、Kanban） |
| `notes/session-names.md` | 会话名与 tmux 名的调研与规则（2026-10-19） |
| `notes/provider-switch-probe.md` | 运行中切换 API 供应商的实测（2026-10-20） |

## 依赖链 & 推进
- S0 立项 ✅（2026-10-18）
- S1 调研 + 设计 ✅：G1 用户批准（2026-10-18），D1–D4 已拍板
- S2 编排：4 Phase × 11 Task + R1，P1 是 MVP（T1.1–T1.4）；G2 已过（2026-10-18）
- S4 实施：P1（T1.1–T1.4）与 P2（T2.1–T2.4）✅（2026-10-19）；P3/P4 待下一轮
- 第一波可并行：T1.1 探针、T1.2 bypass、T2.1 守护进程

## 开发笔记
- 公开仓：档案正文不得出现主机名、内网地址、账号、私有仓名
- 端到端验证用独立状态目录（`CC_MOSS_STATE_DIR`）与独立 tmux socket，不碰真实会话
- 用户在用这台 Mac 时不做鼠标 / 键盘模拟；UI 用离屏渲染与 `--click` 探针
