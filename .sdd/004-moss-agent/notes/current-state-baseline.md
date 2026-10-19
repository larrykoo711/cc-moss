# 004 现状基线：现有能力盘点（R1–R7）

核实日期 2026-10-18，主干 691fcef

判定：✅ 可直接用 / ⚠️ 有但没为这件事接线 / ❌ 没有。证据均为读代码所得，路径相对仓库根。

## R1 新会话默认 bypass（App 与 CLI）

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| `cc-moss new` 拼 claude 命令行 | ✅ | `crates/cc-moss/src/cmd/new.rs:66-75` | argv = `claude --name=<n> --session-id <uuid>` + `config.claude.default_args` + 用户传入的 claude 参数 |
| `[claude] default_args` 配置项 | ⚠️ | `crates/moss-core/src/config.rs:24-29` | 能写 `["--dangerously-skip-permissions"]`，但默认空数组；只能手改 config.toml，`config set` 只认 `ui.language`（`config.rs:133`） |
| `new` 直接收 claude 参数（不必写 `--`） | ✅ | `crates/moss-core/src/launch_args.rs:58-103`，`crates/cc-moss/src/main.rs:307-320` | `new --dangerously-skip-permissions x` 被改写为 `new x -- --dangerously-skip-permissions` |
| 恢复时带回 bypass | ✅ | `launch_args.rs:9-17`（白名单含 `--permission-mode`、`--dangerously-skip-permissions`），`crates/cc-moss/src/cmd/restore.rs:362-364` | SessionStart 时用 `ps` 抓参数（`launch_args.rs:107-118`，`cmd/hook.rs:83-87`），restore 原样追加 |
| tmux 启动方式 | ✅ | `crates/moss-core/src/tmux.rs:174-191` | `new-session -d -P -F … -s <name> -c <cwd> <argv…>`，不经 shell，claude 退出即会话结束 |
| claude 可执行文件定位 | ✅ | `crates/cc-moss/src/cmd/launch.rs:42-72` | `CC_MOSS_CLAUDE_BIN` > PATH > 安装器默认位置，转绝对路径 |
| App 新会话表单 | ⚠️ | `apps/macos/Sources/MossKit/NewSession.swift:57-59`，`apps/macos/Sources/CCMoss/SessionForms.swift:52-78` | 只传 `new <name> -C <dir> --detach [--yes]`，无任何 claude 参数，也无 bypass 开关；只能间接吃 CLI 的 `default_args` |
| 别名「免确认新会话」 | ✅ | `apps/macos/Sources/MossKit/AliasBoard.swift:23`，`crates/cc-moss/src/cmd/alias.rs:458` | 预置任务 `trusted = new --dangerously-skip-permissions`（建议名 `tt`），需用户自己建 |
| `settings.json` 的 `permissions.defaultMode` | ❌ | 全仓 grep `defaultMode`/`bypassPermissions`/`skipDangerous` 无结果 | cc-moss 只改 hooks，不碰权限配置（`crates/moss-core/src/settings_json.rs:1-5`） |
| 测试世界已默认 bypass | ✅ | `crates/cc-moss/tests/common/mod.rs:145-148` | e2e 的 config.toml 写了 `default_args = ["--dangerously-skip-permissions"]` |

## R2 内置 Moss 会话 + Doctor 能力

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| `cc-moss doctor` | ⚠️ | `crates/cc-moss/src/cmd/doctor.rs:43-109,120-460,593,642` | 只查**安装环境**：Claude Code 在否、hooks（current/outdated/incomplete/broken）、缺 pid 的会话数、tmux 版本与 conf、starship、提示符顺序、shell 块、ssh 复位、脚本 PATH、别名链接、版本、App。`--fix` 只修别名链接（`doctor.rs:44-58`） |
| 会话层面的检查/整理 | ❌ | 同上 | 不查重名、长期中断、空会话、无名会话、笔记缺失等 |
| 可供 agent 调用的会话工具 | ✅ | `crates/cc-moss/src/main.rs:34-203` | `ls --json`、`events --json`、`note`（goal/state/note）、`close`、`restore --yes --json`、`send`、`attach` 都是现成 CLI |
| 预置 prompt / 内置会话概念 | ❌ | 全仓无 | 没有「保留名」或「系统会话」标记；`new` 对名字只做唯一性检查（`cmd/new.rs:46-49,104-166`） |
| 一键启动入口 | ⚠️ | `NewSession.swift:57`，`MossClient.newSession`（`NewSession.swift:101-103`） | App 能起一个具名 detach 会话，但没有带初始 prompt 的通道；初始 prompt 只能事后 `send` |
| 无头 claude 会话被忽略 | ⚠️ | `crates/cc-moss/src/cmd/hook.rs:45-53,247-252` | `CLAUDE_CODE_SESSION_ATTENDED=0`（`claude -p`/SDK）不进 registry；Moss 若用无头模式跑，不会出现在列表 |

## R3 代答：会话等待用户时由 Moss 接管

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| 已安装的 hook 事件 | ✅ | `crates/moss-core/src/agent/mod.rs:13-30`，`settings_json.rs:19-28` | 仅 6 个：SessionStart、UserPromptSubmit、Stop、SessionEnd、Notification、PostToolUse（matcher `*`，`settings_json.rs:162-164`）；超时 10 s（`settings_json.rs:16`） |
| PreToolUse / PermissionRequest hook | ❌ | `agent/mod.rs:23-30` | 未注册；而能返回 allow/deny 的正是这类事件 |
| hook 返回决策 | ❌ | `cmd/hook.rs:1-5,217-238` | 只观测；stdout 仅可能输出 `systemMessage`（重名提醒、非 tmux 提醒），从不输出 decision；任何错误吞掉、恒退出 0（`hook.rs:22-34`） |
| 「需要你」判定 | ✅ | `crates/moss-core/src/agent/claude.rs:76-91` | Notification 的 `notification_type`：`permission_prompt`→permission，`elicitation_dialog`/`elicitation_url_dialog`→question，`agent_needs_input`→input |
| idle_prompt | ❌ | `claude.rs:36,83` | `idle_prompt`、`agent_completed`、`auth_success` 被主动丢弃 |
| AskUserQuestion 与权限提示可区分 | ❌ | `claude.rs:77-78`，`.sdd/002-menubar/notes/hook-probe-2.md` N3 | 2.1.301 上二者都是 `permission_prompt`、message 相同；区分需 PreToolUse |
| 时延 | ⚠️ | `hook-probe-2.md` N2，`apps/macos/Sources/MossKit/NotificationPlan.swift:134-136` | 权限提示出现约 6–9 s 后才发 Notification；用户先点了就不会有 |
| 记录内容 | ✅ | `crates/moss-core/src/registry.rs:358-372`，`registry.rs:18-19` | 写 `slots.attention` + 事件 `needs_you {reason, message≤200 字}` |
| 清除标记 | ✅ | `registry.rs:164-170,603-615`，`registry.rs:305,327,335` | PostToolUse 快路径清 attention；Prompt/Stop/End 也清 |
| 题目选项 / 工具入参 | ❌ | `claude.rs:13-32` | Payload 只解析 session_id/cwd/source/reason/title/prompt/notification_type/message，无 tool_input |
| 用按键代答的通道 | ⚠️ | `crates/cc-moss/src/cmd/send.rs:15-57` | 可往 pane 打字+回车，但拒绝控制字符（`send.rs:16-24`），选项菜单所需的方向键/数字键语义未验证 |
| 气泡里的同意/拒绝位 | ⚠️ | `apps/macos/Sources/CCMoss/PetBubble.swift:182-185`，`PetPrototype.swift:391,697` | 消息框结构里预留「004 的 approve / deny」区，尚未实现 |

## R4 常驻守护会话 + 系统级守护进程

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| 后台守护进程 / launchd 服务 | ❌ | 全仓 grep `launchctl`/`KeepAlive`/`daemon` 无结果 | 唯一常驻的是菜单栏 App 本身 |
| 登录启动 App | ✅ | `apps/macos/Sources/CCMoss/LoginItem.swift:20-47`，`crates/cc-moss/src/cmd/app.rs:669` | `SMAppService.mainApp`，失败退回 `~/Library/LaunchAgents/<label>.plist`（`open -g`，`RunAtLoad`，无 KeepAlive） |
| 进程消失检测 | ✅ | `crates/cc-moss/src/reconcile.rs:16-47`，`apps/macos/Sources/CCMoss/AppModel.swift:103-108` | 无守护进程：`ls`/picker 打开时 reconcile 记 interrupted；App 每 60 s 跑一次 `ls --json` 兜底 |
| 自动拉起中断会话 | ⚠️ | `cmd/restore.rs:362-386`，`main.rs:152-165` | `restore --yes [names]` 能恢复，但没有谁自动调用；App 只在通知/面板上提示 |
| 防误杀 | ⚠️ | `crates/cc-moss/src/cmd/close.rs:14-24`，`crates/cc-moss/src/pick/mod.rs:270-283` | `close` 拒绝关运行中会话；但 picker 的「结束」会 `kill-session`，无保护名单 |
| 状态目录 | ✅ | `crates/moss-core/src/store_dir.rs:12-31` | `$CC_MOSS_STATE_DIR` > `$XDG_STATE_HOME/cc-moss` > `~/.local/state/cc-moss`；内有 `registry.db`（SQLite WAL）、`hook.log`、`update.json`、`.logged-*`、临时 `send-<pid>.txt` |

## R5 Moss 调度：派发 bypass 会话、选目录、拿不准问用户

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| 程序化建会话 | ✅ | `cmd/new.rs:30-102` | `new <name> -C <dir> --detach -- <claude 参数>`，固定 session id 便于 hook 首事件匹配（`new.rs:64-65`） |
| 名字唯一与冲突处理 | ✅ | `cmd/new.rs:104-228` | 同名运行中→进入；中断→询问恢复（`--yes` 跳过）；空会话→重开 |
| 目录候选 | ⚠️ | `NewSession.swift:28-44`，`NewSession.swift:47` | App 有「最近目录」与「工作区」设置，CLI 侧没有目录推荐 |
| 首条任务下发 | ⚠️ | `cmd/send.rs:15-57` | 只能建完再 `send`；多行走 bracketed paste（`send.rs:59-84`）；会话须 Running 且在 tmux |
| 改名 | ✅ | `apps/macos/Sources/MossKit/Message.swift:17-19`，`cmd/hook.rs:125-133,165-204` | 通过 `send "/rename x"`，UserPromptSubmit hook 同步 tmux 名 |
| 向用户提问的 UI | ❌ | — | 无对话/输入框；只有通知与气泡的单一按钮（`NotificationPlan.swift:21-34`） |
| 监听派发结果 | ✅ | `cmd/events.rs:1-67`，`registry.rs:20-23` | `events --json --since <seq>`：started/working/finished/needs_you/interrupted/restored/renamed/closed/ended，保留 7 天 |

## R6 外部渠道 Slack App

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| 任何 IM / webhook / 网络监听 | ❌ | 全仓无 | 无 HTTP server、socket、Slack 依赖 |
| 唯一联网 | ⚠️ | `crates/moss-core/src/update.rs:1-2,17-18,102-104` | 每天一次查 GitHub Releases 新版本；`[update] check=false` 或 `CC_MOSS_NO_UPDATE_CHECK` 关闭 |
| 人设文案基础 | ⚠️ | `apps/macos/Sources/MossKit/Catchphrases.swift`，`.sdd/002-menubar/notes/design-voice.md` | 有 Mo 的口吻规范与口头禅，可借给外部人设 |
| 可复用的消息模型 | ⚠️ | `NotificationPlan.swift:58-77`（`SessionAlert`），`AlertRouter.swift:18-24` | 渠道无关的 alert 结构；但 `AlertDestination` 只有 notification / pet / none |

## R7 Mo ↔ 外部渠道路由 + 在场检测

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| 「通知与 Mo 只取其一」 | ✅ | `apps/macos/Sources/MossKit/AlertRouter.swift:48-77` | 每条 alert 按 seq 只决定一次；勿扰或面板开着→none；auto 下 Mo 可见→pet，否则→通知 |
| 路由设置 | ✅ | `AlertRouter.swift:4-16`，`apps/macos/Sources/CCMoss/NotificationSettings.swift:9-18` | `alertMode`：auto / notifications / pet |
| 路由接线点 | ✅ | `apps/macos/Sources/CCMoss/StatusItem.swift:290-316`，`apps/macos/Sources/CCMoss/Notifier.swift:114-197` | `AlertCenter(context:toPet:)`，`EventFeed` 推给 pet 与 alerts；新渠道可在此加 destination |
| 锁屏 / 熄屏检测 | ✅ | `apps/macos/Sources/CCMoss/PetController.swift:609-620,320,92` | 监听 `com.apple.screenIsLocked/Unlocked`、`screensDidSleep/Wake`、窗口遮挡；只用于判定 Mo 可见 |
| 空闲（无键鼠）检测 | ❌ | 全仓 grep `CGEventSource`/`HIDIdle` 无结果 | — |
| 勿扰 | ✅ | `PetController.swift:20,94-96` | `petDoNotDisturbUntil` 时间戳 |
| 声音策略 | ✅ | `apps/macos/Sources/MossKit/Voice.swift:30-63` | VoicePolicy：勿扰静音、30 s 至多一声、按 PetInteraction 决定哪些 kind 出声，只念标题 |
| 交互强度 | ✅ | `apps/macos/Sources/MossKit/PetDirector.swift:6-19` | quiet / everyday / active |
| 通知可执行动作 | ⚠️ | `Notifier.swift:22`，`NotificationPlan.swift:18,21-34` | 每条通知只一个按钮（Open / Bring Back），无文本输入、无同意/拒绝 |

## 事件管道与测试设施（横切）

| 能力 | 判定 | 证据 file:line | 说明 |
|------|------|----------------|------|
| App 获取事件 | ✅ | `apps/macos/Sources/MossKit/Watcher.swift:4-37`，`AppModel.swift:139,155-160`，`apps/macos/Sources/CCMoss/EventFeed.swift:5-64` | FSEvents 监视 `ls --json` 给出的 `state_dir`（`cmd/ls.rs:27`）→ `refresh()` → `events --json --since` 增量；无推送、无常驻流 |
| App 调 CLI | ✅ | `apps/macos/Sources/MossKit/Runner.swift:46-47`，`apps/macos/Sources/MossKit/Client.swift:23-26` | 每次 `Process` 起 cc-moss 子进程 |
| 打开会话 | ✅ | `apps/macos/Sources/MossKit/LaunchPlan.swift:3-40` | 终端里跑 `cc-moss attach <handle>` |
| hook 性能预算 | ✅ | `crates/cc-moss/tests/hook_perf.rs:11-37` | p95 < 30 ms |
| 隔离 e2e 世界 | ✅ | `crates/cc-moss/tests/common/mod.rs:128-165,227-241` | 独立 tmux socket（`-L cc-moss-test-<pid>-<test>`）、临时目录、`CC_MOSS_STATE_DIR`/`CC_MOSS_CONFIG`/`CC_MOSS_TMUX_SOCKET`/`CC_MOSS_CLAUDE_BIN`；结束时 kill-server |
| 假 claude | ✅ | `crates/cc-moss/tests/fixtures/fake-claude` | 解析 `--name/--session-id/--resume`，经 FIFO 接受 prompt/clear/exit 并发 hook；不模拟权限提示与 hook 返回值 |
| 默认 HOME 隔离 | ✅ | `common/mod.rs:13-41` | `HOME=/nonexistent/...`、关更新检查、固定语言 |
| App 离屏渲染 | ✅ | `apps/macos/Sources/CCMoss/CCMossApp.swift:38-60` | `--pet-sheets [--bubbles]`、`--pet-probe`、`--bench-*` |

## 对设计的提示

1. **hook 全是旁观者**：6 个事件只写 registry，stdout 只会出 `systemMessage`，恒退出 0（`cmd/hook.rs:1-5,22-34`）。R3 代答需要新增能阻塞并返回决策的事件（PreToolUse / PermissionRequest），且要打破「hook 永不失败、毫秒级」契约（`hook_perf.rs` p95<30 ms，`settings_json.rs:16` 超时 10 s），这是架构级变化。
2. **needs-you 来自 Notification，晚 6–9 秒、且分不清**：`permission_prompt` 同时覆盖权限与 AskUserQuestion（`claude.rs:77-80`）；`idle_prompt` 被丢弃（`claude.rs:36`）。它适合「提醒」，不适合「代答」。
3. **needs-you 不带题目内容**：只存 reason + ≤200 字 message（`registry.rs:18,358-372`），无选项、无 tool_input；代答 UI 必须从新 hook 的 payload 拿结构化题目。
4. **bypass 只有一个旋钮**：CLI 的 `[claude] default_args`（默认空，`config.rs:24-29`）；App 表单不传任何 claude 参数（`NewSession.swift:57-59`）。R1 最小改动是改默认值或在 `new.rs:74` 处注入，并给 App 一个开关；restore 已能带回 bypass（`launch_args.rs:17`）。
5. **没有守护进程**：中断检测靠 `ls` 时 reconcile + App 60 s 定时（`reconcile.rs:16-47`，`AppModel.swift:103-108`）；登录项只是 `open -g` App，无 KeepAlive（`LoginItem.swift:37-47`）。R4 的系统级守护与「杀不死的 Moss 会话」都是新建；picker 的结束键会直接 `kill-session`（`pick/mod.rs:282`），需保护名单。
6. **无头会话不入册**：`ATTENDED=0` 的 hook 被跳过（`hook.rs:45-53`）。Moss 若用 `claude -p`/SDK 跑，不会在面板出现；若要可见、可恢复，就得是 tmux 里的交互会话。
7. **向会话输入只有 `send`（tmux send-keys / paste-buffer）**：禁控制字符、须 Running 且在 tmux（`send.rs:15-57`）；改名即 `send "/rename x"`（`Message.swift:17`）。用它驱动选项菜单不可靠，代答应走 hook 返回值而非模拟按键。
8. **路由已有「一条 alert 只去一处」的单点**：`AlertRouter`（`AlertRouter.swift:48-77`）+ `AlertCenter`（`StatusItem.swift:290-316`），R7 加外部渠道应扩展 `AlertDestination` 而不是另起一套；但它活在 App 进程里，App 不运行时没有路由。
9. **在场检测只做了一半**：锁屏/熄屏/遮挡已监听（`PetController.swift:609-620`），但只服务「Mo 是否可见」；无键鼠空闲检测，也未上提为全局 presence 状态。
10. **当前零外部通道**：唯一联网是每日版本检查（`update.rs:17-18,102-104`）。R6 引入 Slack 会首次带来长连接、令牌与对外隐私承诺，guide 与官网的「网络使用」表述需同步改。
