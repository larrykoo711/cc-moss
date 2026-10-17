# 002: feature — Stage 2 macOS 状态栏 App

## 来源
- 无 tracker，序号 002（见 `.sdd/config.md` `{EDITION}`）
- 触发：Stage 1（`001-cli`，v0.1.1）收尾，用户 2026-10-16 启动 Stage 2

## 需求摘要
- 缺口（证据见 `notes/drivers-evidence.md`）：
  - D1 会话状态只有打开终端敲 `t` 才看得到
  - D2 重启后没有任何东西提醒「有会话断了」
  - D3 去某个会话得先找终端窗口
  - D4 用户原始需求：macOS 原生常驻状态栏 App
- 性质：能力建设 epic，Stage 2
- 边界：只做 macOS；App 只经 CLI 读写，不碰 `registry.db`；不自动恢复（沿用 R8）

## 改动范围
- 新增 `apps/macos/`（Swift Package）
- CLI 小接口：`ls --json` 加 `project`/`worktree` 字段
- 打包与发布：`release.yml`、Homebrew 公式模板、`cc-moss install`/`update`

## 目录索引
| 文件 | 用途 |
|------|------|
| `notes/drivers-evidence.md` ⭐ | 立项依据、Stage 1 接口、外部事实（终端脚本接口、签名与 Homebrew 政策） |
| `notes/current-state-baseline.md` | App 需要的能力在 CLI 里的现状（✅/⚠️/❌） |
| `DESIGN-menubar.md` ⭐ | 权威设计：假设 A1–A12、capability map、面板草图、终端适配、测试、开放问题 |
| `diagrams/architecture-menubar.html` | 架构图 |
| `diagrams/sequence-open-session.html` | 开机提醒 → 找回 → 在终端打开的时序 |
| `diagrams/src/` | 两张图的 archify 源 JSON |
| `PLAN.md` ⭐ | 3 Phase × 10 Task、依赖图、Kanban、日志、开放问题 |
| `notes/probes.md` | 探针结论：Swift Testing、通知要求 App 在应用程序目录、SwiftUI 宏需 Xcode 工具链 |
| `notes/hook-probe-2.md` | Notification / PostToolUse 钩子真机探针 |
| `reports/T1.1-manual.md` | T1.1 真机走查与用户反馈处置记录 |
| `reports/delivery.md` ⭐ | 收尾交付报告：指标、批次、四桶 |
| `notes/design-voice.md` | 用户可见文案的规则（已沉淀到仓库根 `CLAUDE.md`） |
| `playground.html` | 可视化档案（背景、缺口、方案、Kanban） |

## 依赖链 & 推进
- S0 立项 ✅、G1 设计批准 ✅（2026-10-16）
- S2 编排完成：3 Phase × 10 Task，P1 是 MVP（T1.1–T1.3），待 G2
- 第一波 ✅：T1.1（Swift 面板，#14）与 T2.1（CLI 事件流，#15）
- 2026-10-16 重排为 4 Phase × 16 Task：P3 桌面宠物并入本档案（亮点），发布顺延 P4
- 第二波：T1.2、T2.2、T2.6、T3.1 四路 agent 并行
- 后续 epic（另立档案）：003-pet、004-pet-approve、005-voice，见 `DESIGN-menubar.md` §1.1
- ✅ 2026-10-17 收尾：16 个任务全部 Done，0.2.0 发布（#21）。交付报告 `reports/delivery.md`；
  宠物已并入本档案 P3，后续只剩 004-pet-approve、005-voice

## 开发笔记
- 公开仓：档案正文不得出现主机名、内网地址、账号、私有仓名
- 端到端验证用独立状态目录（`CC_MOSS_STATE_DIR`）与独立 tmux socket，不碰真实会话
