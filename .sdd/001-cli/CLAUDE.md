# 001: feature — Stage 1 终端版：可恢复的 Claude Code 会话 + 终端工具套件

## 来源
- 无 tracker，序号 001（见 `.sdd/config.md` `{EDITION}`）
- 触发：2026-10-15 一次机器重启后，进行中的多个 Claude Code 会话只能靠人工翻会话记录来定位和恢复

## 需求摘要
- 缺口（证据见 `notes/drivers-evidence.md`）：
  - D1 重启后会话无法定位和恢复
  - D2 tmux 会话和 Claude 会话没有对应关系
  - D3 会话没有备注
  - D4 终端标准资产不是一套可安装的工具
- 性质：能力建设 epic。新开源项目的 Stage 1
- 边界：Stage 2（macOS 状态栏 App）另立 `002-menubar`，本档案只负责为它留好可复用的核心库与机读接口

## 改动范围
- 新仓 `cc-moss`（Rust）
- 从私有终端标准 skill 迁入 tmux.conf、starship.toml、`t` 会话工具，迁入后本仓是正本；
  原 skill 只保留私有的多机下发部分，改为调用本仓的安装命令
- 锚点：`notes/code-anchors-verified.md`（S1 产出）

## 目录索引
| 文件 | 用途 |
|------|------|
| `notes/drivers-evidence.md` ⭐ | 立项依据、已有能力、开源调研、已定的设计方向 |
| `notes/hook-probe.md` ⭐ | Claude Code 钩子实测 F1–F9（状态机的事实依据） |
| `notes/current-state-baseline.md` | 待迁入资产的现状盘点、14 条坑、脱敏要求 |
| `DESIGN-cli.md` ⭐ | 权威设计：已拍板项 R1–R9、capability map、数据模型、状态机、命令、测试、风险 |
| `diagrams/architecture-cc-moss.html` | 架构图 |
| `diagrams/sequence-hook-lifecycle.html` | 一个槽位从新建到恢复的钩子时序 |
| `diagrams/src/` | 两张图的 archify 源 JSON |
| `PLAN.md` ⭐ | 4 Phase × 12 Task、依赖图、Kanban、日志、开放问题 |
| `playground.html` | 可视化档案（背景、缺口、方案、Kanban） |
| `LESSONS.md` | 踩坑沉淀（L1–L9） |
| `reports/T1.1-review-1.md` | T1.1 两轮独立评审与处置 |
| `reports/T1.4-review-1.md` | T1.4 两轮独立评审与处置 |

## 依赖链 & 推进
- G1 已批准（2026-10-15）；S2 编排完成：4 Phase × 12 Task，P1 是 MVP（T1.1–T1.4）
- 第一波：T1.1；T1.1 完成后 T1.2 与 T1.3 并行；关键路径 T1.1 → T1.3 → T1.4 → T2.1 → T3.1 → T3.2 → T4.1
- P1 MVP ✅（2026-10-15）：T1.1–T1.4 全部 Done
- T1.5 会话名唯一 ✅（2026-10-16）
- T2.1 选择器 ✅（2026-10-16）：`t` → `cc-moss pick`
- 下一步：用户真机走查 `t`；之后 T3.1（终端资产 install/doctor）

## 开发笔记
- 公开仓：档案正文不得出现主机名、内网地址、账号、私有仓名
- 端到端测试一律用独立 tmux socket 和临时 HOME，不得碰正在用的 tmux server 与真实的 `~/.claude`
