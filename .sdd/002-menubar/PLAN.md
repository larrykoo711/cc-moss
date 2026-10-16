# PLAN — 002-menubar：cc-moss Stage 2 macOS 状态栏 App（4 Phase × 16 Task）

## 配套文档

| 文档 | 用途 |
|------|------|
| `DESIGN-menubar.md` | 权威设计（假设 A1–A16 已定，产品路线 §1.1） |
| `notes/drivers-evidence.md` | 立项依据、终端脚本接口、签名政策、Claude Code 钩子事实 |
| `notes/current-state-baseline.md` | App 需要的能力在 CLI 里的现状 |
| `diagrams/` | 架构图、开机提醒 → 找回 → 打开的时序图 |

## 铁律

- 每个任务先有 spec 且过 `sdd-gate.sh`，再动代码；实施在 `feature/002-<task>` 分支上，PR squash 合并
- Rust 改动：`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --all` 绿；
  Swift 改动：`swift build` + `swift test`（`apps/macos/`）绿
- App 只经 `cc-moss` 读写，绝不打开 `registry.db`（单一写入方）
- 测试与演示用独立状态目录（`CC_MOSS_STATE_DIR`）与独立 tmux socket，不碰真实会话
- 公开仓：提交前用私有词表扫描新增内容与 `git log -p origin/main..HEAD`
- 宿主事实见 `.sdd/config.md`

## 总览

| Phase | 主题 | Task | 价值 |
|-------|------|------|------|
| P1 | MVP：一眼看清，点一下进去，断了能找回 | T1.1–T1.3 | 解决 D1–D3：不开终端就知道状态，点一下进会话，重启后一键找回 |
| P2 | 动态、通知与设置 | T2.1–T2.6 | 事件流地基；图标动画、通知中心、开机提醒、面板里的日常操作 |
| P3 | 桌面宠物（这一版的亮点） | T3.1–T3.4 | 透明置顶的小伙伴把会话事件演出来：动画、气泡、语音播报 |
| P4 | 发布 | T4.1–T4.3 | 随 cc-moss 一起装、一起升；首次启动引导；0.2.0 发版并装上 3 台 Mac |

## 依赖图

```text
T1.1 面板与快照 ─┬─> T1.2 在终端里打开 ─┬─> T1.3 找回
                 │                      ├─> T2.5 新会话/便条
                 │                      └─> T2.3 通知中心 ─> T2.4 开机提醒与设置补齐
T2.1 事件流(CLI) ┴─┬─> T2.2 图标动画 ──────────┐
                   ├─> T3.1 宠物设计 ─> T3.2 宠物窗口与动画 ─> T3.3 气泡 ─> T3.4 语音
T2.6 原生别名（CLI 可独立先行）                └──────────────┘
P1–P3 全部 ─> T4.1 打包与分发 ─> T4.2 首次启动引导 ─> T4.3 打磨、文档与 0.2.0
```

- 关键路径：T2.1 → T3.1（设计待批）→ T3.2 → T3.3 → T3.4 → T4.1 → T4.2 → T4.3
- 并行窗口：第二波 T1.2、T2.2、T2.6（CLI）与 T3.1（宠物设计）四路并行；T3.1 设计需用户批准后才进 T3.2
- P1 完成后本机先装开发版真实用一段时间（MVP 验收点），再进 P2 的视觉动画

## 任务

### T1.1 面板与快照

- 目标：菜单栏出现 cc-moss 图标，点开是精心设计的面板：搜索、按项目分组、每会话两行（名字 + 状态 + 时间 / 最后一句或便条），实时刷新
- 文件：`crates/cc-moss/src/cmd/ls.rs`（加 `project`、`worktree`）、`apps/macos/Package.swift`、
  `apps/macos/Sources/MossKit/*`、`apps/macos/Sources/CCMoss/*`、`apps/macos/Tests/MossKitTests/*`、`.github/workflows/ci.yml`
- 内容：先做两个探针（Q5 只用命令行工具能否构建与测试；Q6 ad-hoc 签名能否申请通知权限），结论写入 notes；
  `ls --json` 加字段；MossKit（找 cc-moss、补 PATH、可注入的命令执行、解码、状态与分组、过滤）；
  Watcher（目录变化合并 1 秒 + 60 秒兜底 + 打开面板时刷新）；MenuBarExtra 面板；找不到 cc-moss 的引导卡；
  CI 加 macOS Swift 任务
- TDD：解码（字段缺失、未知 schema）、状态计算、分组与排序（对齐选择器）、过滤、路径探测
- 验收：独立状态目录 + 演示数据下面板显示与 `t` 一致；钩子写入后 2 秒内面板更新
- commit：`feat(app): menubar panel with live sessions`
- 依赖：无

### T1.2 在终端里打开

- 目标：点会话即在用户选的终端里新开窗口进入它；设置里可选终端（默认系统终端，推荐 Ghostty）
- 文件：`apps/macos/Sources/MossKit/Terminal*.swift`、`apps/macos/Sources/CCMoss/*`
- 内容：Terminal（默认）/ Ghostty（推荐，≥1.3）/ iTerm2 / WezTerm / Kitty / Alacritty 适配；设置里「终端」一项列出已安装的，Ghostty 未装时给推荐与下载入口；「复制命令」兜底；
  名字单引号转义；自动化授权被拒的引导；不在 tmux 的会话不给打开
- TDD：各终端生成的脚本（表驱动，含引号、空格、中文名）；探测优先级；版本判断
- 验收：三种终端真机各打开一次；名字含单引号也能正确进入
- commit：`feat(app): open a session in your terminal`
- 依赖：T1.1

### T1.3 找回

- 目标：面板顶部「有 N 个会话断了 · 全部找回」，单行「找回」，找回后可直接打开
- 文件：`apps/macos/Sources/MossKit/Actions.swift`、`apps/macos/Sources/CCMoss/*`
- 内容：调用 `cc-moss restore --yes [name…]`；进行中状态、成功后刷新、失败原样展示 CLI 的错误
- TDD：命令拼装、错误解析、进行中禁止重复点击
- 验收：独立 socket 上杀掉 tmux server 后从面板全部找回，`ls` 显示 running
- commit：`feat(app): bring interrupted sessions back`
- 依赖：T1.1、T1.2

### T2.1 会话事件流（CLI）

- 目标：`cc-moss events --json --since <序号>` 能增量读到会话发生了什么；`ls --json` 有 `attention`
- 文件：`crates/moss-core/src/{registry,event,agent/*,settings_json}.rs`、`crates/cc-moss/src/cmd/{hook,events,setup,doctor}.rs`、`crates/cc-moss/src/reconcile.rs`
- 内容：`events` 表（只增、7 天清理）；钩子写事件（started / working / finished / needs_you / ended / renamed）；
  对账写 interrupted，`restore` 写 restored；注册 `Notification`、`PostToolUse` 钩子并据此置/清 `attention`；
  `setup` 补钩子、`doctor` 提示缺失；钩子仍永远 exit 0
- TDD：事件序列表驱动（取真实钩子输入）；序号单调；清理边界；`attention` 置与清；旧库迁移
- 验收：钩子 p95 < 30ms（含新钩子，Q7）；`setup` 在已装旧钩子的临时 HOME 上只补新增两项
- commit：`feat(core): session event stream and attention`
- 依赖：无（与 T1.1 并行；`ls.rs` 冲突由后合入方变基解决）

### T2.2 图标动画

- 目标：菜单栏图标用动画表达动态（A7）
- 文件：`apps/macos/Sources/CCMoss/StatusIcon*.swift`、`apps/macos/Resources/*`
- 内容：Continuum 模板图；四态与优先级；图标旁紧凑数字与显示方式（A18）；在忙 = 光点流动、需要你确认 = 轻跳 + 强调色；减少动态效果、屏幕休眠时停帧
- TDD：状态 → 图标态的优先级映射；帧调度在休眠/减少动态时暂停
- 验收：四态真机录屏；空闲时 App CPU < 1%
- commit：`feat(app): an icon that shows what is going on`
- 依赖：T1.1、T2.1

### T2.3 通知中心

- 目标：会话事件按类型发到系统通知中心，点通知打开该会话
- 文件：`apps/macos/Sources/MossKit/Notify*.swift`、`apps/macos/Sources/CCMoss/*`
- 内容：订阅事件流；类型开关与默认值（A9）；同一会话短时间内合并；点击 = 在终端打开；权限被拒时的引导；**经 MossKit `AlertRouter` 决定出口（A23：自动 / 只用通知中心 / 只用小莫，一件事只提醒一次）**
- TDD：事件 → 通知的映射、开关、合并窗口
- 验收：需要你确认、断了、做完了、找回完成四类真机各触发一次
- commit：`feat(app): session events in Notification Center`
- 依赖：T1.2、T2.1

### T2.4 开机自启与设置

- 目标：开机自启（首次问一次）；开机发现有断了的会话就提醒；设置窗口按 DESIGN A20 参照 Raycast（通用 / 通知 / 别名 / 关于四个标签页，含唤出面板的全局快捷键）
- 文件：`apps/macos/Sources/CCMoss/{Settings,Lifecycle}*.swift`
- 内容：`SMAppService` 登录项；启动时对账并提醒；设置持久化
- TDD：首次启动流程状态机；设置读写
- 验收：真机重启一次，登录后收到「有 N 个会话断了」，点开即可找回
- commit：`feat(app): start at login and settings`
- 依赖：T2.3

### T2.5 新会话、便条与不再提醒

- 目标：在面板里开新会话（选目录 + 起名）、写便条、不再提醒
- 文件：`apps/macos/Sources/MossKit/Actions.swift`、`apps/macos/Sources/CCMoss/*`
- 内容：`cc-moss new <名字> -C <目录> --detach` 后在终端打开；`note`；`close`；重名时按 T1.5 规则提示
- TDD：命令拼装；重名处理
- 验收：三项操作真机各走一次，`t` 里同步可见
- commit：`feat(app): new sessions and notes from the menubar`
- 依赖：T1.2

### T2.6 原生别名

- 目标：别名是 `~/.local/bin` 里指向 `cc-moss` 的符号链接，按 `argv[0]` 查 `config.toml` 的 `[aliases]` 执行；`cc-moss alias ls|add|rm`；`t` 迁到这套机制；设置页可增删改（A19）
- 文件：`crates/moss-core/src/{config,aliases}.rs`、`crates/cc-moss/src/{main,cmd/alias,cmd/setup,cmd/install,cmd/doctor}.rs`、`apps/macos/Sources/*/Aliases*.swift`
- 内容：argv[0] 分派；别名命令解析（只允许调用 cc-moss 子命令，不经 shell）；链接指向稳定入口；重名与遮挡检查（Q8–Q10）；`function t` 退役（setup 升级时移除旧函数、建 `t` 链接）；doctor 检查；App 设置页的别名列表（经 CLI 读写）
- TDD：argv[0] 分派；参数拼接与引号；名字校验；PATH 重名拒绝；遮挡检测；旧 `function t` 迁移；链接失效修复
- 验收：bash 与 zsh 下 `t`、自定义别名都能用；改别名后新命令立即生效；`install --uninstall` 删掉全部链接
- commit：`feat(alias): native aliases, editable from the menubar`
- 依赖：T1.1（设置窗口骨架已在 T1.1 建好）；CLI 部分无依赖，可并行

### T3.1 宠物设计

- 目标：桌面宠物的设计档（DESIGN 附录）经用户批准：形象与风格、动作表（待机 / 在忙 / 需要你 / 断了 / 开心）、与事件的对应、气泡与语音的规则、交互（拖动、点击、右键菜单、勿扰）、性能与隐私预算；产出形象的原型帧与一张交互原型图
- 内容：调研同类（桌面宠物、Mac 上的常驻小助手）；形象自绘（矢量 / 帧动画），不使用任何第三方受版权保护的形象
- 验收：用户批准设计（G1）
- 依赖：T2.1

### T3.2 宠物窗口与动画

- 目标：透明、无边框、置顶、可拖动、记住位置的宠物小窗，按会话状态播放对应动作；设置里可开关、可调大小
- 依赖：T3.1、T2.2

### T3.3 气泡消息

- 目标：会话事件以气泡出现在宠物旁（需要你确认、做完了、断了、找回了），点击气泡在终端打开会话；同类合并、勿扰时段；三种互动模式（安静 / 日常 / 积极，DESIGN-pet §6.5：先表情动作示意，悬停或点击才展开消息框）；与通知中心共用 `AlertRouter`，不重复提醒（A23）
- 依赖：T3.2、T2.3、T1.2

### T3.4 语音播报

- 目标：本机语音合成播报关键事件（中英随界面语言），可开关、可调音量与音色，勿扰时静音
- 依赖：T3.3

### T4.1 打包与分发

- 目标：`brew install`/`brew upgrade` 带上 App，`cc-moss install` 放进「应用程序」，`cc-moss update` 一起升
- 文件：`apps/macos/scripts/bundle.sh`、`.github/workflows/release.yml`、`packaging/homebrew/*`、`crates/cc-moss/src/cmd/{install,update,doctor}.rs`
- 内容：组装 `.app`（`LSUIElement`、图标、版本号取自 workspace）、ad-hoc 签名、Release 产物与校验和；
  公式在 macOS 上装入 App；`install` 询问后复制到 `~/Applications`；`update` 替换 App 并重启它；`doctor` 检查 App 版本
- TDD：版本号一致性；install/update 对 App 的处理（临时 HOME）
- 验收：干净的 macOS 用户目录下 `brew install` → `cc-moss install` → App 能启动并显示会话
- commit：`feat(release): ship the menubar app with cc-moss`
- 依赖：P1、P2、P3 全部

### T4.2 首次启动引导

- 目标：首次打开 App 的欢迎窗口（DESIGN A22）：介绍、检测并引导安装 CLI 与钩子、开机自动打开与通知授权、默认终端
- 文件：`apps/macos/Sources/CCMoss/Onboarding*.swift`
- 内容：多步欢迎窗口（进度点、上一步 / 下一步、可跳过）；每步做实事（调 `cc-moss doctor`/`install`、`SMAppService`、`UNUserNotificationCenter`、终端探测）；只在首次或手动从设置里重新打开
- TDD：步骤状态机（完成、跳过、回退、已满足的步骤自动打勾）
- 验收：干净用户目录下首次启动，走完引导后面板可用、通知可达；中英各走一遍
- commit：`feat(app): a first-run welcome`
- 依赖：T4.1

### T4.3 打磨、文档与 0.2.0

- 目标：中英文案与视觉走查（human-feel），README/指南加上 App，发 0.2.0，装上 3 台 Mac
- 文件：`apps/macos/Sources/CCMoss/Localizable*`、`README*.md`、`docs/guide*.md`、`docs/assets/*`、`CHANGELOG.md`
- 验收：E2E human-feel 走查记录（每个界面中英各过一遍，没有单语言文字）；3 台 Mac 经 `cc-moss update` 升到 0.2.0 并启动 App
- commit：`docs: the menubar app` + `chore: release 0.2.0`
- 依赖：T4.2

## Kanban

| Task | 仓 | 依赖 | 状态 | SDD | 验收锚点 |
|------|----|------|------|-----|----------|
| **T1.1** 面板与快照 | cc-moss | — | ✅ Done | done/T1.1-panel-and-snapshot.spec.md | 面板与 `t` 一致，2 秒内刷新 |
| **T1.2** 在终端里打开 | cc-moss | T1.1 | ✅ Done | done/T1.2-open-in-terminal.spec.md | 默认终端与 Ghostty 真机打开、设置可切换 |
| **T1.3** 找回 | cc-moss | T1.1,T1.2 | ✅ Done | done/T1.3-bring-back.spec.md | 杀 server 后面板全部找回 |
| **T2.1** 会话事件流（CLI） | cc-moss | — | ✅ Done | done/T2.1-event-stream.spec.md | 事件序列 + 钩子 p95 |
| **T2.2** 图标动画 | cc-moss | T1.1,T2.1 | ✅ Done | done/T2.2-icon-motion.spec.md | 四态录屏，空闲 CPU < 1% |
| **T2.3** 通知中心 | cc-moss | T1.2,T2.1 | ✅ Done | done/T2.3-notifications.spec.md | 四类通知真机触发 |
| **T2.4** 开机自启与设置 | cc-moss | T2.3 | ✅ Done | done/T2.4-startup-and-settings.spec.md | 重启后收到提醒并找回 |
| **T2.5** 新会话、便条与不再提醒 | cc-moss | T1.2 | ✅ Done | done/T2.5-new-and-notes.spec.md | 三项操作 `t` 里可见 |
| **T2.6** 原生别名 | cc-moss | T1.1 | ✅ Done | done/T2.6-native-aliases.spec.md | bash/zsh 下别名可用，改完即生效 |
| **T3.1** 宠物设计 | cc-moss | T2.1 | ✅ Done | done/T3.1-pet-design.spec.md | 用户批准宠物设计 |
| **T3.2** 宠物窗口与动画 | cc-moss | T3.1,T2.2 | ✅ Done | done/T3.2-pet-window.spec.md | 透明置顶、按状态播放动作 |
| **T3.3** 气泡消息 | cc-moss | T3.2,T2.3,T1.2 | ✅ Done | done/T3.3-pet-bubbles.spec.md | 事件气泡、点击打开会话 |
| **T3.4** 语音播报 | cc-moss | T3.3 | 🟨 In Progress | — | 中英播报、勿扰静音 |
| **T4.1** 打包与分发 | cc-moss | T1.3,T2.2,T2.4,T2.5,T2.6,T3.4 | 🟨 In Progress | — | 干净用户目录 brew 全流程 |
| **T4.2** 首次启动引导 | cc-moss | T4.1 | ⬜ Backlog | — | 干净用户目录首次启动走完引导 |
| **T4.3** 打磨、文档与 0.2.0 | cc-moss | T4.2 | ⬜ Backlog | — | 3 台 Mac 升到 0.2.0 |

## 日志

- 2026-10-16 S0 立项；S1 设计草案 → 用户回复「是的，状态栏 App 是一个精心设计的面板……」并追加：图标动画表达动态、
  session 事件经通知中心通知、核心完成后做桌面宠物（动画 + 气泡 + 语音、代办确认、语音对话），整个 App 用 sdd 驱动。
  据此加入 A13–A16 与产品路线 §1.1（003-pet、004-pet-approve、005-voice 各自另立档案）。G1 通过
- 2026-10-16 Q1 分发方式：按建议 A12（随 cc-moss 一起发、ad-hoc 签名、不走 cask），用户未提异议；办开发者账号后再加公证与 cask，不推翻本条
- 2026-10-16 S2 编排：3 Phase × 10 Task，P1 为 MVP
- 2026-10-16 G2：用户「开工吧」。探针 Q5（用 Swift Testing，不用 XCTest）、Q6（ad-hoc 签名可用通知，但 App 须在应用程序目录）结论见 `notes/probes.md`；O1、O2 关闭
- 2026-10-16 S3：T1.1、T2.1 spec 过 gate → todo/；设计里的 `project` 字段取消（分组键直接用 `cwd`），改为 `ls --json` 加 `state_dir`

- 2026-10-16 AMEND T1.1：用户指定面板视觉与交互参照 Raycast（DESIGN A17），加强项，记入 spec 澄清日志
- 2026-10-16 用户指定状态栏条目参照 iStat Menus（DESIGN A18）：T1.1 加摘要条；T2.2 增加图标旁紧凑数字与显示方式（仅图标 / 图标 + 数字）
- 2026-10-16 用户提议：别名在客户端设置里自定义，并做成原生命令而非写入 shell 的函数 → DESIGN A19（BusyBox 式符号链接 + argv[0] 分派）、新增 T2.6；`t` 一并迁移
- 2026-10-16 用户指定设置窗口参照 Raycast 设置（DESIGN A20）：T2.4 的设置窗口与 T2.6 的别名表格都按此实现；新增「唤出面板的全局快捷键」
- 2026-10-16 用户要求动作栏有悬停/点击反馈、logo 悬停动起来并点击进入设置 → 在 T1.1 里提前搭好设置窗口骨架（通用：cc-moss 路径；关于：版本、检查更新）；通知、别名等标签页随 T2.3、T2.6 加入，T2.4 补齐其余通用项
- 2026-10-16 设置窗口二改：用户对照本机 Raycast 截图指出差距 → 读取本机 Raycast 设置页样式表取度量（只取数值，不复制资源），改为系统侧边栏 + 分组行（DESIGN A20 已更新）；顺带提前做了「开机自动打开」
- 2026-10-16 用户要求做 Raycast 式首次启动引导，正式版发布前做 → DESIGN A22、新增 T4.2，原 T4.2 顺延为 T4.3
- 2026-10-16 用户：P2 做完还不到打包发布的程度，P3 要有桌面宠物这个亮点 → 原 003-pet 并入本档案为 P3（T3.1–T3.4），原 P3 发布顺延为 P4（T4.1–T4.3）；代办确认（004）与语音对话（005）仍是后续 epic
- 2026-10-16 用户：T1.2 的终端在设置里可配置多种，默认系统终端，推荐 Ghostty → DESIGN A5 更新
- 2026-10-16 用户：「补充完善 kanban 然后继续全力推进 with sdd and agent team」
- 2026-10-16 T2.1 合入 main（#15，独立评审 2 Important + 2 Minor 均已修复并补红→绿测试，380 测试全绿，钩子 p95 PostToolUse 3.6ms / Notification 7.4ms）；T1.1 变基后全绿，随 #14 合入
- 2026-10-16 T1.1 合入 main（#14）。第二波 S3：T1.2、T2.2、T2.6、T3.1 spec 过 gate，派 agent team 四路并行（各自 worktree 与 feature 分支，controller 负责看板与合并）
- 2026-10-16 T3.1 设计批准：用户「B，小莫」——形象方向 B、名字「小莫 / Mo」；DESIGN-pet §13 其余 9 项（默认开启方式、话量正常、语音默认关、勿扰不破例、跟随专注模式、单击开面板、宠物在场不重复发通知、64pt 右下角、两个彩蛋）按推荐值执行，均可在设置中更改
- 2026-10-17 用户真机反馈修复：点小莫或写便条后 App 退出（最后窗口关闭即退出）、右键菜单改为小莫气泡、提示定位、设置文案精简与说明字调淡；用户「没问题，继续按 sdd kanban 迭代」
- 2026-10-17 第三波 Review 收口：T1.3、T2.3、T2.5、T3.2、T3.3 写入 reports；点击穿透真机探针通过（alpha 保持默认）。各报告「待用户确认」项与 T3.2 录屏改为离屏关键帧（需批准）留待用户验收后进 Done
- 2026-10-17 T2.4 spec 过 gate；Rust `config get/set` 由 agent 并行完成（feature/002-t2.4-config 合入集成分支），Swift 侧开机提醒、全局快捷键 ⌃⌥M、界面语言由 controller 完成；本机 App 改用开发版 CLI（Homebrew 0.1.1 缺新命令）
- 2026-10-17 用户验收「没问题，开始 Ship」：T1.3、T2.3、T2.4、T2.5、T3.2、T3.3 转 Done；各报告「待用户确认」项随日常使用验证，问题走反馈车道
- AMEND T3.2: 「六个动作录屏」改为真实图层离屏关键帧（中英、深浅 12 张）approved-by=user date=2026-10-17
- 2026-10-17 wave three 合入 main（#18，squash 后树与集成分支逐字节一致）
- 2026-10-17 反馈车道（用户：设置面板不够精细、菜单随意、分组不好；便条提示糟糕；改名交互糟糕）：设置拆为 通用 / 菜单栏 / 小莫 / 通知 / 别名 / 关于（细线分组、页头标题、分组标题降级、小莫关闭时收起从属项、小莫头图），程序位置移入关于；表单快捷键改为按钮上的按键标签、说明移到标题右侧；改名加大输入框、预选现名、一键建议、带图标的实时状态（feature/002-settings-ia）
- 2026-10-17 T4.1（打包分发）与 T3.4（语音）各派一名 agent 并行（feature/002-t4.1-ship、feature/002-t3.4-voice）
- 2026-10-16 T2.2 菜单栏图标三轮真机测量：每帧换图 10fps 4.3%、Core Animation 26.5%（WindowServer +13%）、每秒 2 次换图 1.43%（≈ iStat Menus 菜单栏进程 1.3%）。结论：菜单栏每次更新有固定系统开销，只能压更新次数；流畅动画留给 P3 宠物窗口
- 2026-10-16 用户：菜单栏动态效果可关（A24，已实现于集成分支）；系统通知与宠物提醒互斥（A23，T2.3 / T3.3 共用 AlertRouter）
- 2026-10-16 用户修订宠物交互：需要你时不直接弹消息框，先用表情动作（日常模式另加半透明口头禅小气泡）示意，悬停或点击才展开；三种模式安静 / 日常（默认）/ 积极；平时半透明、互动或悬停时过渡到不透明 → DESIGN-pet §6.5；半透明与悬停过渡并入 T3.2，模式与气泡在 T3.3
- 2026-10-16 第二波合入 main（#17）：T1.2、T2.2、T2.6 Done（用户真机确认终端与 Ghostty 都能打开），T3.1 Done；期间按用户反馈完成关于页重做（软件更新 `update --check --json`）、Raycast 式下拉、会话行悬停打开、全局手形指针、菜单栏动态效果开关
- 2026-10-16 第三波：T3.2 小莫离屏 CPU ≤ 0.04%、内存 +5MB；真机开启后 App CPU +0.3%、WindowServer 无增量、footprint 19MB，交互待用户走查。T1.3、T2.3、T2.5 spec 过 gate；两路 agent：A = T1.3 → T2.5（同在面板动作），B = T2.3（含 AlertRouter，T3.3 复用）

## 推进节奏

1. 第一波 ✅ 已完成：T1.1 与 T2.1 并行
2. 第二波（agent team，四路并行，各自独立 worktree）：T1.2 在终端里打开、T2.2 图标动画、T2.6 原生别名、T3.1 宠物设计（设计稿交用户批准）
3. 第三波：T1.3 找回、T2.3 通知中心、T2.5 新会话/便条；T3.2 宠物窗口（T3.1 批准后）
4. 第四波：T2.4 补齐、T3.3 气泡、T3.4 语音
5. 发布：T4.1 → T4.2 → T4.3

## 开放问题

| # | 问题 | 何时定 |
|---|------|--------|
| O1 | ~~Q5~~ 已关闭：命令行工具可构建，测试用 Swift Testing | 2026-10-16 |
| O2 | ~~Q6~~ 已关闭：可以，但 App 须在应用程序目录 | 2026-10-16 |
| O3 | Q7 `PostToolUse` 钩子对每次工具调用的开销 | T2.1 实测 |

## 已定调（防翻案）

- 设计 A1–A16（见 `DESIGN-menubar.md` §2），2026-10-16 用户批准
- 本迭代不拦截任何确认（A16）；代办确认、桌面宠物、语音对话分别在 004、003、005
