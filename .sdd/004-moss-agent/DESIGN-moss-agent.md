# DESIGN：Stage 3 Moss agent

> 状态：S1 草案，待 G1 批准。依据：`notes/drivers-evidence.md`、`notes/current-state-baseline.md`、`notes/reference-study.md`、
> `notes/claude-code-facts.md`、`notes/slack-presence-facts.md`、`notes/anchors.md`。`arch_level: yes`。

## 0. 已拍板（2026-10-18 用户，防翻案）

| # | 决定 |
|---|------|
| D1 | Slack 里 Moss **只在频道里**响应，不接私聊；频道里**所有人权限一样**。Moss agent 照参考项目 main agent 设计：有自己的人设 / 档案 / 日志，管会话，管 skills 与插件 |
| D2 | 新会话 **bypass 直接默认开启**，升级后不再询问 |
| D3 | Moss 常驻会话**一直在线**，空闲不耗额度，有事件才唤醒 |
| D4 | 交付顺序：**先 bypass 和代答**（P1 = MVP），再守护进程 + Moss + Doctor，再派发，最后 Slack 与路由 |

D1 的风险已向用户说明一次：频道成员都能让这台 Mac 跑 bypass 会话。设计里的兜底（不改变「人人平等」）：只认允许的频道、每次派发写审计日志、一键暂停 Moss。

## 1. 一句话

把 cc-moss 从「会话的记事本」升级成「会话的管家」：新会话默认放手干（bypass），停下来等人时由 Moss 替你接住问题、送到你在的地方（桌面或 Slack），并由一个常驻的 Moss agent 体检会话、派发新会话、在 Slack 频道里和团队对话。

## 2. 假设（已按用户回答修订）

| # | 假设 | 依据 |
|---|------|------|
| A1 | bypass 的开关是 `[claude] permission_mode = "bypass" \| "default"`，默认 `bypass`；`cc-moss new --safe` 单次用普通模式；App 设置里可改默认。旧的 `default_args` 照旧追加 | D2；`config.rs:24-29` |
| A2 | 首次 bypass 的警告框会让 tmux 里的会话卡住，所以 `cc-moss install`（含升级时的自动 install）在 `~/.claude/settings.json` 写 `skipDangerousModePermissionPrompt: true`（受管块，可撤销）。管理策略禁用 bypass 时退回普通模式，`doctor` 和新会话表单说明原因 | claude-code-facts §1 |
| A3 | bypass 下没有权限询问，代答主要对象是 **AskUserQuestion（选择题）** 和 **等你输入（idle）**；权限询问只出现在 `--safe` 会话，走 PermissionRequest | claude-code-facts §2 |
| A4 | 代答是**新的、可等待的 hook**（`cc-moss hook ask`），与现有「永不失败、30ms」的观察型 hook 分开立契约：出错或超时一律「不表态」，让会话照常显示自己的提示（fail-open 到原生交互） | `cmd/hook.rs:1-5` |
| A5 | 代答等待默认 300 秒（上限 600），可设。**等待期间终端里的对话框照常能答**，谁先答算谁的；超时即自然回落到会话自己的提示，Moss 那边标「已转回会话」 | `notes/ask-probe.md` Q3、F |
| A6 | 回填用 **PermissionRequest `allow` + `updatedInput.answers`**（`{问题原文: 选中的 label}`），界面显示正常的「User answered」；普通与 bypass 会话都触发。PreToolUse 只用来提早拿题目、立刻退出，不在里面等（它运行时会挡住对话框）。退路：PreToolUse 短等 + `updatedInput` → `deny` 带理由 → 只提醒（K1） | `notes/ask-probe.md`（Claude Code 2.1.302 实测） |
| A7 | 「等你输入」不是阻塞提示，不用 hook 拦：Moss 显示「它在等你」+ 输入框，回复经现有 `cc-moss send` 敲进会话 | baseline R3 |
| A8 | 系统级守护 = **`cc-moss daemon`**（Rust），launchd **用户级** LaunchAgent（`KeepAlive`、`RunAtLoad`，登录后运行）。不用 root：Claude Code 拒绝 root 下 bypass | claude-code-facts §1 |
| A9 | 守护进程是**唯一**拉起 Moss 会话、连 Slack、跑在场检测、做路由的进程；App 降为界面客户端（读状态、写回答），避免两边重复拉起 | reference-study §4 |
| A10 | 「杀不掉」= cc-moss 的界面和命令都不提供结束 Moss 的入口（列表里显示为固定的「Moss」）；被外力杀掉，守护进程按 30s / 2m / 10m / 1h 阶梯重启；只有设置里「关闭 Moss」才真正停 | reference-study §4 |
| A11 | Moss 会话 = tmux 里的交互式 `claude`（bypass），工作目录 `<state>/moss/`，带自己的 CLAUDE.md 人设矩阵、`.claude/settings.json` 防护 hook、skills；每次重启**全新开**，注入「读台账接着干」，身份和进度都在文件里 | reference-study §2、§6 |
| A12 | 往 Moss 送消息用 tmux 粘贴 + 回读确认（复用 `send` 的做法）；取回复用 Stop hook 写完成标记 + 读对话记录认领。对话记录格式无官方文档 → 单独封装、有降级（拿不到就只回「已收到」） | reference-study §2.2 |
| A13 | Moss 不逐条接收所有会话事件：守护进程把「需要你 / 断了 / 做完了」攒成摘要，最快每 30 秒注入一次，省额度 | D3 |
| A14 | Moss 的手只有 cc-moss 命令：派发、体检、改名、写便条、发消息、关闭都经 CLI 执行，CLI 负责策略（bypass、审计、频率、总开关）。Moss 自己的防护 hook 禁止它改自己的 settings / hooks、禁止写工作目录以外的文件 | reference-study §3、§4 |
| A15 | Slack：守护进程用 Socket Mode 直连（无公网地址），token（`xapp-` / `xoxb-`）存钥匙串；设置页给预填好的 manifest 链接一键建 App。一台 Mac 对应一个 Slack App | slack-presence-facts (a) |
| A16 | 在场检测在守护进程里做（LaunchAgent 在登录会话里，能读空闲秒数与锁屏状态，无需授权）：在场 <2 分钟无输入；短暂离开 2–10 分钟；离开 ≥10 分钟或熄屏；锁定 = 锁屏或睡眠。回到在场需解锁或连续输入 3 秒 | slack-presence-facts (b) |
| A17 | 隐私：Slack 只看到问题、选项、Moss 的回复，不转发会话的对话内容。Moss 用本机 Claude Code 登录，不需要 API key | — |
| A18 | 只做 macOS；IM 先只做 Slack；文案中英双语，遵守 `002-menubar/notes/design-voice.md` | 仓库 `CLAUDE.md` |

## 3. Capability map

| # | 能力 | 需求 | 可独立测试的验收 |
|---|------|------|------------------|
| C1 | 新会话默认 bypass | R1 | 临时 HOME 下 `cc-moss new x` 的 claude 命令行带 bypass；`--safe` 不带；App 表单同；install 写入 `skipDangerousModePermissionPrompt`；管理策略禁用时退回并有说明 |
| C2 | 代答中枢（ask broker） | R3 | 假 claude 触发 PreToolUse(AskUserQuestion) → `<state>/asks/<id>.json` 出现完整选项 → `cc-moss ask answer` → hook 返回答案；超时 / 出错不表态；先答者胜 |
| C3 | 桌面答题 | R3 | 离屏渲染：单选、多选、带自由输入、允许 / 拒绝四种卡片；面板与小莫气泡都能答；设置「替会话接住问题」开关 |
| C4 | `cc-moss daemon` | R4 | launchd 加载后 `kill -9` 自动回来；单实例锁；`cc-moss daemon status`；App 退出不影响它 |
| C5 | Moss 常驻会话 + 档案 | R4/R5 | 守护进程拉起 `moss` 会话；被杀后按阶梯重启并读台账恢复；列表里不可结束；人设 / 日志 / 记忆 / skills 文件由 install 布置、升级只覆盖受管文件 |
| C6 | Doctor | R2 | `cc-moss doctor --sessions --json` 给出事实；Moss 的 doctor skill 给建议；破坏性动作经 C2 先问；面板一键「让 Moss 体检」 |
| C7 | 派发会话 | R5 | `cc-moss dispatch` 起一个 bypass 会话并注入任务；目录由 `dispatch resolve` 排出候选，拿不准经 C2 问；做完回报来源；每次写审计日志；总开关能暂停 |
| C8 | Slack 渠道 | R6 | 只在允许的频道响应；@Moss 的消息进 Moss、回复回到同一线程；人设名字 / 头像；C2 的问题渲染成 Block Kit 按钮 / 输入框 |
| C9 | 路由与在场 | R7 | 状态机单测（在场 / 短暂离开 / 离开 / 锁定）；设置「交给桌面 / Slack / 自动」；绑定 Slack 后桌面安静；小莫「回到桌面」；两边任一处答了，另一边撤回 |

## 4. 架构

```
Claude Code 会话（tmux，bypass）
   │ 观察型 hook（现有 6 个，快）        │ 代答 hook（新：PreToolUse AskUserQuestion / PermissionRequest，可等）
   ▼                                      ▼
registry + events（现有）            <state>/asks/<id>.json  ⇄  <state>/asks/<id>.answer.json
   │                                      ▲              ▲              ▲
   ▼                                      │              │              │
cc-moss daemon（launchd 用户级，唯一的常驻进程）
   ├─ 守护 Moss 会话（拉起 / 判活 / 阶梯重启 / 注入消息 / 认领回复）
   ├─ 事件摘要 → Moss（≥30s 一批）
   ├─ 在场检测 + 路由（桌面 / Slack / 自动）
   └─ Slack Socket Mode（频道消息 ⇄ Moss；问题 ⇄ Block Kit）
        ▲                    ▲
cc-moss.app（界面客户端）   Moss 会话（tmux 里的 claude，<state>/moss/）
 面板答题卡 / 小莫气泡       人设矩阵 + 防护 hook + skills（doctor / dispatch / sessions / profile / skills）
 设置：bypass、代答、        它的手：cc-moss dispatch / doctor / rename / note / send / close（CLI 执行并审计）
 Moss、Slack、路由
```

- **文件即接口**：沿用现有约定（App 经 CLI 读写、FSEvents 监视状态目录），新加 `asks/`、`moss/`、`daemon/`（pid、状态、日志）、`audit.jsonl`。写回答用 `O_EXCL` 创建保证先答者胜。
- **代答 hook 的流程**：会话要问 → hook 判断「代答已开 且 有接手方（App 在运行或守护进程在线）」，否则立刻不表态 → 写 ask → 等回答（kqueue 盯文件，≤ 超时）→ 按 A6 的方式回填 → 删除 / 归档 ask。会话被用户在终端里直接答了 → PostToolUse 到达 → ask 标「已在会话里回答」，各处撤回。
- **路由**：每个 ask 产生时按当前在场状态定去处；状态变化时重新路由未答的 ask（例如人走开 10 分钟，桌面上没答的问题转到 Slack）。
- **Moss 的任务分级**（照参考项目）：L0 寒暄可静默；L1 60 秒内能查完的直接答；L2 超过 60 秒或要产出文件的一律派发；L3 复杂的先派一个会话拆解。L2 规则由防护 hook 强制，不靠自觉。

## 5. Moss 档案（`<state>/moss/`）

| 文件 | 用途 | 谁能改 |
|------|------|--------|
| `CLAUDE.md` | 用 `@` 依次引入下面各文件 | 受管（install 覆盖） |
| `SECURITY.md` | 最高优先级：频道外不说话、不泄露路径与内部机制、不碰 token、破坏性动作先问 | 受管 |
| `IDENTITY.md` / `SOUL.md` | 名字、头像、说话方式（默认「Moss」，冷静可靠，取自《流浪地球》） | 只能经 `/profile` 改 |
| `USER.md` | 团队与用户偏好 | `/profile` |
| `SESSIONS.md` | 当前会话一览（守护进程从 registry 生成） | 生成 |
| `journal/YYYY-MM-DD.md` | 每日日志：做了什么、派了什么、问了谁 | Moss 自己 |
| `memory/`、`contacts/` | 长期记忆、频道里的人 | Moss 自己 |
| `.claude/settings.json` | 防护 hook：禁止改自身 settings / hooks、禁止写目录外文件、L2 产出强制派发 | 受管 |
| `.claude/skills/` | `moss-doctor`、`moss-dispatch`、`moss-sessions`、`moss-profile`、`moss-skills`（装 / 卸 skills 与插件，先快照再改） | 受管；用户自装的不动 |

## 6. Doctor（C6）
- 事实：`cc-moss doctor --sessions --json` 列出断了很久的、重名的、没名字的、不在 tmux 的、worktree 已删的、tmux 里还在但 registry 认为结束的、hook 不全的。
- 判断：`moss-doctor` skill（预制 prompt）分「可直接整理」（补名字、写便条、归档很久以前断的）和「要问你」（关闭、删除、改别人的东西）。
- 执行：一律经 cc-moss 命令；要问的走 C2；结果写进日志并回报。
- 入口：面板「让 Moss 体检」、Slack「@Moss 体检一下」、守护进程每天一次（可关）。

## 7. 派发（C7）
- `cc-moss dispatch --task <文件> [--dir <目录>] [--name <名字>] [--from <来源>]`：起 bypass 会话、注入任务、写 `audit.jsonl`（谁、来自哪个频道、目录、任务摘要、时间）。
- 选目录：`cc-moss dispatch resolve "<任务>"` 给出候选（任务里提到的路径或项目名 → 工作区 → 最近活跃的项目 → 工作区下的 git 仓库），带分数；分数不够或前两名接近 → 经 C2 让用户选（选项 = 候选目录 + 「别的目录」）。
- 做完：被派会话的 Stop hook → 守护进程告诉 Moss → Moss 回报到来源（频道线程 / 桌面）。
- 总开关：设置与 Slack 里都能「暂停 Moss 接活」，暂停时 `dispatch` 直接拒绝。

## 8. Slack（C8）与路由（C9）
- 绑定流程（设置 › Slack）：打开预填 manifest 的创建链接 → 安装到工作区 → 粘贴两把 token → 选「允许的频道」和「提醒发到哪个频道」→ 测试消息。
- 人设：名字 / 头像在 manifest 与设置里配；消息头带频道与发言人，Moss 据此称呼人。
- 问题渲染：单选 → 按钮（≤25）或下拉；多选 → 复选框（≤10）或多选下拉；自由输入 → 消息内输入框；允许 / 拒绝 → 两个按钮。点击 3 秒内 ack，再写回答文件。
- 路由设置：「交给谁」= 桌面 / Slack / 自动（默认，未绑定 Slack 时等于桌面）。自动：在场或短暂离开 → 桌面；离开 ≥ N 分钟（默认 10）或锁定 → Slack。绑定 Slack 且当前去 Slack 时，桌面安静：小莫不冒泡，只在菜单栏图标上显示待答数。小莫菜单「回到桌面」= 固定交给桌面，直到再改。

## 9. 测试策略
- 单测：配置解析、bypass 参数拼装、ask 文件协议（先答者胜、超时、撤回）、目录候选打分、在场状态机、Block Kit 渲染、回复控制标记解析。
- 端到端：扩展假 claude（`crates/cc-moss/tests/fixtures/fake-claude`）能发 PreToolUse 并读 hook 返回；独立 tmux socket + 临时 HOME + `CC_MOSS_STATE_DIR`。
- 探针（真 Claude Code，P1 第一个任务）：AskUserQuestion 的两种回填方式；bypass 下 PermissionRequest 是否触发；hook 超时后会话是否回到原生提示。
- App：离屏渲染 + `--click` / `--command` 探针；不模拟用户屏幕上的鼠标键盘。
- Slack：Socket Mode 协议用录制的消息信封做单测；真 Slack 联调在开发用工作区。

## 10. 风险与开放问题
| # | 风险 / 问题 | 处置 |
|---|-------------|------|
| K1 | AskUserQuestion 回填方式随 Claude Code 升级失效 | T1.1 已实测可行（2.1.302）；T1.3 加版本探测与退路链，全部失效时退到「通知 + 一键打开会话」 |
| K2 | 对话记录格式无官方文档，Claude Code 升级可能破坏认领回复 | 认领逻辑单独封装 + 版本探测 + 降级为「已收到」 |
| K3 | 常驻 Moss 与派发会话耗额度 | 摘要批处理、L0 静默、派发有频率上限与总开关 |
| K4 | D1：频道成员都能派发 bypass 会话 | 允许频道白名单、审计日志、一键暂停（已向用户说明） |
| K5 | 锁屏键无 Apple 文档 | 锁屏与睡眠多信号合并，实机在 macOS 14 / 15 / 26 验证 |
| K6 | 一个 Slack App 多台 Mac 同时连会漏消息 | 一台 Mac 一个 App；检测到同一 App 另有连接时提示 |
| Q1 | 版本号：P1 发 0.3.0，之后每个 Phase 一个小版本？ | 默认如此，S2 定 |
