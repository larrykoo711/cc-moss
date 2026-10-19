# 004 立项依据（drivers + 证据）

> 核实日期 2026-10-18，主干 `691fcef`（v0.2.2）。公开仓：参考的私有 agent 项目只写作「参考项目」，不写仓库名。

## 用户原始需求（2026-10-18，逐条）

| # | 需求 | 性质 |
|---|------|------|
| R1 | 新会话默认 bypass，App 与命令行都是 | 缺口 |
| R2 | 内置 Moss 会话，先做 Doctor：预制 prompt，一键开始检测 / 整理 / 修复 / 规范会话，由 Claude Code 自己的 agent 能力驱动 | 新能力 |
| R3 | 可选设置：会话停下等用户输入或做选择题时，把这个事件拦下来交给 Moss，用重新设计的选择框 / 输入框答完，不必进会话；这套 hook 要内置，以后接远程 IM 还要用 | 新能力（原规划 004-pet-approve 并入） |
| R4 | Moss 自己有一个常驻、杀不掉的 Claude Code 守护会话，管理各会话、监听各会话的 hook 并和用户交互；需要一个系统级守护进程管理这些内置的东西；参考项目做过同类事，学习并移植其模块 | 新能力（架构级） |
| R5 | Moss agent 相当于参考项目的 main agent：几乎不执行具体任务，有固定范围的任务；能派发新的 bypass Claude 会话去做事，自己判断在哪个目录启动，拿不准让用户选 | 新能力 |
| R6 | 首个外部渠道：Slack App，Moss 绑定到 Slack App，有自定义人设，同事在 Slack 里和 Moss 对话 | 新能力 |
| R7 | 交互路由：hook 交互委托给外部渠道还是桌面宠物要有明确设置；绑定 Slack 后桌面安静；能在宠物上切回桌面；更智能：需要交互时检测用户在不在（屏幕活跃 / 锁屏），持续不在 N 分钟自动转到已绑定的 Slack | 新能力（设计交给我们） |

## 证据

### R1：新会话现在不是 bypass
- `crates/moss-core/src/config.rs:24-29`：`ClaudeConfig.default_args: Vec<String>`，`#[derive(Default)]` → 默认空列表；注释举例 `["--dangerously-skip-permissions"]` 需用户自己写进配置
- `apps/macos/Sources/MossKit/NewSession.swift`：`arguments(name:directory:)` = `["new", name, "-C", directory, "--detach"]`，不带权限参数
- `apps/macos/Sources/MossKit/AliasBoard.swift:23`：「trusted」别名 `new --dangerously-skip-permissions` 是可选的预设别名，不是默认
- 测试夹具 `crates/cc-moss/tests/common/mod.rs:147` 专门写入 `default_args`，反证默认没有

### R2–R7：现状待 S1 盘点
- 现有 hook、通知、宠物、`cc-moss send`、`cc-moss doctor`、登录项与守护进程的现状，在 `notes/current-state-baseline.md`（S1）逐项给 ✅ / ⚠️ / ❌ 与 file:line
- 参考项目的模块学习在 `notes/reference-study.md`（S1）
- Claude Code 的外部事实（bypass 参数与首次确认、能代答的 hook、无头模式）在 `notes/claude-code-facts.md`（S1）
- Slack 与 macOS 在场检测的外部事实在 `notes/slack-presence-facts.md`（S1）

## 复发检查
- `.sdd/archive/` 不存在，无完结档案可比对
- 相关历史：`002-menubar/DESIGN-menubar.md` §1.1 把「宠物代办确认」规划为后续 epic 004-pet-approve；本档案 R3 覆盖它，原 005-voice 顺延
