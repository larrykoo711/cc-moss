# DESIGN — cc-moss Stage 2（macOS 状态栏 App）

状态：G1 已批准（2026-10-16，用户：「是的，状态栏 App 是一个精心设计的面板……」，并追加图标动画、通知中心与 §1.1 的后续路线）。依据：`notes/drivers-evidence.md`、`notes/current-state-baseline.md`、
`../001-cli/DESIGN-cli.md` §8。

## 0. 沿用 Stage 1 的已拍板项

R4（macOS + Linux，本迭代只做 macOS）、R6（档案公开）、R7（名字以 Claude 会话名为准）、
R8（不自动恢复，恢复由用户触发）、R9（一个 tmux 会话只跑一个 claude），以及「单一写入方」：
只有 CLI 与钩子写 `registry.db`，App 一律经 CLI 读写。

## 1. 一句话

菜单栏里常驻一个 cc-moss 图标：一眼看到哪个会话在忙、哪个在等你、哪个断了；点一下就在你的终端里打开它；
重启之后它会提醒你「有 3 个会话断了」，点一下全部找回。

## 1.1 产品路线（Stage 2 的全部范围，每个 epic 各自走完整 sdd 生命周期）

| Epic | 内容 | 前置 |
|------|------|------|
| **002-menubar**（本档案） | 精心设计的面板；状态栏图标动画表达动态（在忙、在等你、断了、需要你确认）；会话事件发到系统通知中心；**会话事件流**（钩子记事件，App 订阅）作为后续一切的地基 | Stage 1 |
| 003-pet | 桌面宠物：透明置顶的小窗，动画 + 气泡消息 + 语音播报（本地合成），把会话事件演出来；可开关 | 002 的事件流 |
| 004-pet-approve | 宠物代办确认：拦截会话里需要你确认的操作（权限请求等），在宠物气泡里显示完整内容，你点同意/拒绝；超时或 App 不在时退回终端里正常确认 | 002 事件流 + 003 + 本机进程间通道 |
| 005-voice | 和宠物语音对话：宠物说完 → 提示音 → 开始录音 → 说完自动断句 → 本机语音识别 → 送进会话；可随时打断、按住说话作为备选 | 003、004 |

002 的设计因此多一条约束：**事件是一等公民**。通知、图标动画、以后的宠物都是同一条事件流的不同「演出方式」，
不各自去对比快照猜发生了什么。

## 2. 已定的假设

| # | 假设 | 理由 |
|---|------|------|
| A1 | 用 **Swift + SwiftUI**（`MenuBarExtra` 窗口样式），最低 **macOS 14** | 「原生」最直接的做法；SwiftUI 14 起有 `@Observable`，`SMAppService` 做开机自启 |
| A2 | 代码放本仓 `apps/macos/`，Swift Package 构建（不建 Xcode 工程），CI 加一个 macOS 任务跑 `swift build` + `swift test` | 和 CLI 同仓同版本，`ls --json` 契约改动一个 PR 两边一起改 |
| A3 | App 只调用 `cc-moss`：读用 `ls --json`，写用 `restore` `note` `close` `new --detach`；不直接打开 SQLite | 单一写入方；JSON 有版本号、向后兼容，SQLite 表结构不是公开契约 |
| A4 | 刷新：监听状态目录里 `registry.db*` 的变化（钩子每次写都会触发），合并 1 秒内的变化后刷新一次；另每 60 秒兜底刷新一次（用来发现进程已死的会话）；打开面板时立即刷新 | 实测一次 `ls --json` 0.2 秒；按事件刷新比固定轮询省电 |
| A5 | 「打开会话」= 在你的终端里开一个新窗口执行 `cc-moss attach <名字>`。支持 **Ghostty、iTerm2、Terminal**，自动选已安装的（优先级 Ghostty > iTerm2 > Terminal），设置里可改；不支持的终端提供「复制命令」 | 三者都有 AppleScript 可开窗执行命令（Ghostty 1.3 起）；Warp、VS Code 内置终端没有稳定接口 |
| A6 | 不在 tmux 里的会话照样列出，标注「不在 tmux 里，没法从这里打开」，不给打开按钮 | `attach` 对它们本来就做不到 |
| A7 | 图标用定稿的 Continuum 无限环（模板图，跟随菜单栏深浅色），**用动画表达动态**：有会话在忙 = 环上一个光点缓缓流动；需要你确认 = 轻轻跳动并带强调色；在等你 = 静止加小圆点；有会话断了 = 琥珀色静止。优先级：需要你确认 > 断了 > 在忙 > 在等你。系统打开「减少动态效果」时只换颜色不动；屏幕休眠时停帧 | 一眼看状态是 D1 的核心；动画要克制，不抢注意力 |
| A8 | 开机自启默认**开**（首次启动时问一次）；开机时如果发现有中断的会话，发一条系统通知「有 N 个会话断了」，点通知打开面板 | D2：重启后主动提醒，但恢复仍由用户点（R8） |
| A9 | 会话事件发到系统通知中心，按类型开关：**需要你确认**（权限请求、提问）默认开；**会话断了** 默认开；**这一轮做完了，在等你** 默认关（每轮都会触发，太吵）；**找回完成** 默认开。点通知 = 在终端里打开该会话 | 用户追加需求：各种 session 事件经通知中心通知 |
| A13 | **会话事件流**：钩子把每个有意义的变化记成一条事件（新会话、开始干活、这一轮做完、需要你确认、断了、结束、找回、改名），存在登记表的 `events` 表里（只保留最近 7 天）；`cc-moss events --json --since <序号>` 按序号增量读。App 收到「登记表变了」就去拉新事件。快照（`ls --json`）负责「现在是什么样」，事件负责「刚发生了什么」 | 通知和动画需要的是「变化」，对比快照会漏掉 1 秒内一闪而过的变化；003–005 都建在这条流上 |
| A14 | 「需要你确认」的来源：新注册 `Notification` 钩子（`permission_prompt`、`elicitation_dialog`、`agent_needs_input`），置会话的 `attention`；下一次 `PostToolUse`/`Stop`/`UserPromptSubmit` 时清除。`ls --json` 加 `attention` 字段 | Claude Code 文档：Notification 在权限提示等约 6 秒后触发，不能阻塞，只做记录正合适；终端里点了同意没有专门事件，只能靠工具跑完来推断 |
| A15 | 新增钩子要用户重跑一次 `cc-moss setup`（`doctor` 会提示缺哪个），升级时 `cc-moss update` 结束后主动提醒；不偷偷改 `settings.json` | 沿用 Stage 1「改用户配置先给看、再确认」 |
| A16 | 本迭代**不拦截**任何确认：钩子只记录、立即返回，不影响 Claude Code 的正常提示。代办确认是 004-pet-approve 的事 | 拦截涉及安全（误点同意即执行命令），要单独设计与评审 |
| A10 | 中英双语，跟随系统语言，文案沿用 CLI 的语气（口语、友好、带点幽默，不用 emoji） | 与 Stage 1 一致 |
| A11 | App 名 `cc-moss`（Dock 不显示，只在菜单栏），Bundle ID `io.github.larrykoo711.cc-moss` | 与项目同名；不用私人域名 |
| A12 | **随 cc-moss 一起发**（Q1 按建议执行，用户未提异议；改为签名公证 + cask 是后加项，不推翻本条）：同一个版本号、同一个 Release；Homebrew 公式在 macOS 上把 App 一起装进来，`cc-moss install` 问你要不要放进「应用程序」，`cc-moss update` 升级时 App 一起升 | 不新增分发渠道；CLI 与 App 永远同版本，`ls --json` 不会对不上 |

## 3. Capability map

| Module id | 职责 | 依赖 |
|-----------|------|------|
| `cli-contract` | `ls --json` 补 `project`（分组键）、`worktree`、`attention` 字段（schema 1 只加不改） | — |
| `event-stream` | `events` 表 + 钩子写事件 + 对账写「断了」事件 + `cc-moss events --json --since` + 7 天清理；`Notification`/`PostToolUse` 钩子注册 | `cli-contract` |
| `moss-kit` | Swift 库：找到 `cc-moss`、执行命令（可注入，便于测试）、解码快照与事件、算状态与分组、过滤 | `cli-contract` `event-stream` |
| `watcher` | 监听状态目录变化 + 60 秒兜底 + 打开面板时刷新，产出最新快照与新事件 | `moss-kit` |
| `menubar-ui` | 状态栏图标动画（A7）、弹出面板（搜索、分组列表、两行会话卡片、恢复横幅、底栏） | `watcher` |
| `terminal-open` | 终端适配：Ghostty / iTerm2 / Terminal 的开窗脚本，自动探测，复制命令兜底 | `moss-kit` |
| `actions` | 恢复（全部/单个）、新会话、备注、不再提醒；执行后刷新，失败把 CLI 的错误原样展示 | `moss-kit` `terminal-open` |
| `notify` | 把事件按类型开关发到通知中心（A9），点通知打开会话；同一会话短时间内的重复事件合并 | `watcher` `terminal-open` |
| `lifecycle` | 开机自启、开机发现中断时提醒、设置窗口 | `watcher` `actions` `notify` |
| `packaging` | 组装 `.app`（Info.plist `LSUIElement`）、ad-hoc 签名、Release 产物、公式带 App、`install`/`update` 接管 App | 以上全部 |

依赖单向无环。图见 `diagrams/architecture-menubar.html`、`diagrams/sequence-open-session.html`。

## 4. 面板长相（草图）

```text
 ┌───────────────────────────────────────────┐
 │  搜索会话…                                 │
 │  ! 有 2 个会话断了             [全部找回]   │
 │                                           │
 │  projects                                 │
 │   ● term-kit           在忙   · 刚刚       │
 │     开始 Ship Stage2                       │
 │   ○ api-review         在等你 · 5 分钟前    │
 │     这个接口的错误码对一下                  │
 │  billing  · worktree                      │
 │   ◌ fix-invoice        已中断 · 昨天  [找回] │
 │     便条：停在对账脚本                      │
 │  ───────────────────────────────────────  │
 │  新会话…                 设置    退出      │
 └───────────────────────────────────────────┘
```

- 点会话行 = 在终端里打开；行尾「⋯」= 写便条 / 不再提醒；已中断的行点击 = 找回并打开
- 键盘：打开面板即可打字搜索，上下选，回车打开，Esc 关闭（与 `t` 一致，不需要记别的）
- 新会话：选目录（默认最近用过的项目）+ 起名，建好后在终端里打开

## 5. 找到 cc-moss

按顺序找：设置里手动指定的路径 → `/opt/homebrew/bin/cc-moss` → `/usr/local/bin/cc-moss` →
`~/.cargo/bin/cc-moss` → `~/.local/bin/cc-moss`。找不到时面板只显示一张引导卡（怎么安装，一条命令）。
版本低于 App 要求时同样提示升级。

## 6. 打开终端

| 终端 | 做法 |
|------|------|
| Ghostty | `new surface configuration` 设 `command` 为 `cc-moss attach <名字>`，`new window with configuration` |
| iTerm2 | `create window with default profile command "…"` |
| Terminal | `do script "…"`，再 `activate` |

- 命令里的名字一律按 shell 单引号转义；脚本经 `NSAppleScript` 执行，首次会触发系统的「自动化」授权弹窗，
  被拒时给出去系统设置的引导
- 该会话已经在某个终端窗口里打开时，仍新开一个窗口（tmux 允许多个客户端）；「切到已有窗口」留作后续

## 7. 测试策略

- `moss-kit`：JSON 解码（含字段缺失、schema 不认识）、状态计算、分组排序、过滤、路径探测、
  各终端生成的脚本与转义（表驱动）。命令执行层可注入假实现，不碰真实 `cc-moss`
- `cli-contract`：Rust 侧给新字段加测试，锁定 schema 1 只增不改
- UI：SwiftUI 预览 + 演示数据渲染截图；真机走查（human-feel）覆盖图标三态、打开、找回、新会话
- 端到端：用独立状态目录（`CC_MOSS_STATE_DIR`）和独立 tmux socket 跑演示数据，不碰真实会话

## 8. 风险与开放问题

| # | 项 | 处理 |
|---|----|------|
| Q1 | **分发与签名**：没有 Apple Developer ID 证书，App 无法公证；经浏览器或 Homebrew cask 下载的未公证 App 会被 Gatekeeper 拦下 | 建议 A12：App 随 Homebrew 公式 / `cc-moss` 自己的下载安装，CI 做 ad-hoc 签名（Apple Silicon 运行所需），不走 cask。将来若办了开发者账号，再加公证与 cask。**需用户拍板** |
| Q2 | GUI 进程的 PATH 极简（`/usr/bin:/bin:/usr/sbin:/sbin`） | 已实测（2026-10-16）：这样的 PATH 下 `ls --json` 正常；但 `restore`/`new` 要按名字调用 `tmux`（`moss-core/src/tmux.rs:57`），会找不到。App 调用 `cc-moss` 时把它所在目录和 Homebrew 两个 bin 目录补进 PATH。新终端窗口里跑的是用户的登录 shell，不受影响 |
| Q3 | Ghostty 低于 1.3 没有脚本接口 | 探测版本，低于 1.3 时退回 Terminal 并提示 |
| Q4 | 「自动化」授权被用户拒绝 | 显示引导和「复制命令」，不反复弹 |
| Q6 | ad-hoc 签名的 App 能否申请通知权限（`UNUserNotificationCenter`） | T1.1 第一步实测；不行就只用面板内提醒和图标动画，并把结论记入 LESSONS |
| Q7 | `PostToolUse` 每次工具调用都会跑一次钩子 | 钩子 p95 < 30ms 的硬约束不变，实测超标则改为只在 `Stop`/`UserPromptSubmit` 时清 `attention`（代价：确认后到这一轮结束前，图标仍显示「需要你确认」） |
| Q5 | 只用命令行工具（未选 Xcode）能否构建 SwiftUI App、跑测试 | T1.1 第一步实测；CI 用 GitHub 的 macOS 镜像（自带 Xcode） |
