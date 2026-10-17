# 设计语气与设置页规范（design-voice）

> 用途：重做设置窗口（通用 / 菜单栏 / 小莫 / 通知 / 别名 / 关于）里的每一行，包括图标和中英文案。中文是主语言，英文是写给英文用户的另一版，不是翻译过来的。
> 来源编号 [n] 见文末。调研日期 2026-10-17。

## 一、原则（8 条）

1. **像一个靠谱的朋友在说话，不像软件在汇报。** 原因：Claude 的定位是「一个碰巧懂行的好朋友」，把人当成能自己做决定的成年人 [1]。所以文案说的是这件事对你有什么用，不讲它在技术上怎么实现。
2. **诚实优先于讨好。** 原因：Anthropic 要求 Claude「在得体的前提下诚实，而不是为了得体放弃诚实」，不制造错误印象 [1]。所以会晚几秒、默认关着、需要权限这类事要说清楚，不吹「秒到」「零打扰」。
3. **安静，不刷存在感。** 原因：Claude 应当只在像一个关心你的朋友那样的意义上「吸引人」，不追求让人上瘾 [1][2]。默认值偏安静：「这一轮做完了」默认关，小莫平时半透明。
4. **平静、稳，不慌张。** 原因：Claude 的性格要求从安全感而不是焦虑出发，保持沉着 [1]。所以出问题时先说「进度都在」，再说怎么办，不用感叹号和「哎呀」。HIG 也说 oops 这类语气词显得不真诚 [3]。
5. **标签直白，可爱留给图标和小莫。** 原因：HIG 说标签别耍小聪明，「Send」比「Let's do it!」好用 [3]。所以标签保持朴素，温度交给图标颜色、说明句和小莫的台词。
6. **只说开着的时候会怎样。** 原因：HIG 说设置说明写「打开后会怎样」就够了，关掉的情况用户自己能推出来 [3]。所以每行一句，写打开后的效果。
7. **默认值替用户做好选择，设置越少越好。** 原因：HIG 要求默认值让大多数人不用改，并尽量少放设置 [4]。Things 也认为「简单很难」，用约束换简单 [9]。用不上的项就藏起来（例如关了出声以后，不显示「嗓音」）。
8. **要权限之前先讲为什么，在用得到的时候才要。** 原因：HIG 要求不在启动时无故要权限，说明文字写成具体、主动语态、带句号的完整句子 [5]。引导流程要短、可以跳过，教学放进真实操作里 [6]。
9. **温暖但克制的视觉。** 原因：Claude 的品牌用暖色（陶土橙 #D97757、米白底），在一片冷蓝的 AI 产品里显得有人情味 [7][8]。所以品牌橙只给小莫用，其余分组用系统色，色块要少而准。

## 二、文案规范

### 2.1 标签（行标题）
- 词性：中文用名词短语或很短的动宾短语，2 到 6 个字，比如「开机自动打开」「嗓音」。不用问句，不用「设置 xx」「启用 xx」这种前缀。
- 英文：不超过 4 个词。
- 同一类东西在全应用里叫同一个名字（HIG：建一张术语表 [3]）。固定叫法：**会话**、**面板**、**小莫 / Mo**、**找回 / bring back**、**断了 / stopped**、**在等你 / waiting for you**。
- 选项值也要短：「只显示图标 / 图标加数字」，不要写成「仅图标模式」。

### 2.2 说明句（标签下的灰字）
- 中文一行，最多 22 个字（目标 14 到 18 个字），英文最多约 60 个字符。写不下就说明这一行在做两件事，应该拆成两行。
- 一句话，写的是对你有什么效果或什么时候发生。中文句末加句号；英文是完整句子时加句号。
- 不写「点击此处」「如需…请…」。
- 需要讲一次「为什么」的地方（权限、默认关着），各用一句话讲清楚。

### 2.3 语气与人称
- 称呼对方用「你」，不用「您」；英文用 you。
- 不用「我们」。HIG 明确说「we」指代不清，在错误提示里尤其是这样 [3]。软件说到自己时用 cc-moss，或者干脆不写主语。
- 设置页里的说明一律是 cc-moss 在说话，说到小莫时用第三人称「小莫」。只有小莫自己开口时（气泡、试听台词、欢迎页）才用「我」。
- 少用「你的」：写「工作区」就够了，不写「你的工作区」（HIG [3]）。
- 不用感叹号。不用「哎呀 / 糟糕 / Oops」。不卖萌堆叠词。

### 2.4 禁用词与替代说法

| 别出现 | 换成 |
|---|---|
| 钩子、hook、注入、接入 | 不提。真要提就说「cc-moss 已经连上 Claude Code」 |
| tmux、server、socket、后台进程、daemon、launchd | 「会话」「在后台替你守着会话」 |
| starship、prompt、状态栏插件 | 不出现在设置页 |
| CLI、命令行、二进制 | 「终端里的 cc-moss」 / "cc-moss in Terminal" |
| 命令行参数、flag、`cc-moss restore` | 「点一下就找回」；命令只放在悬停提示或「复制命令」按钮里 |
| config、配置文件、JSON、plist | 「设置」 |
| 路径细节 `~/…/bin/cc-moss`、目录 | 「位置」「文件夹」；完整路径只放在悬停提示或「在访达中显示」里 |
| 进程、PID、exit code | 「会话」「Claude」 |
| 事件、payload、AlertRouter | 「提醒」「消息」 |
| 失败、错误、无效、异常（单独出现时） | 说清发生了什么、接下来怎么办 |
| 请、您、亲 | 直接说，用「你」 |

### 2.5 中英各自的写法
- 中文：全角标点；中文与英文或数字之间加一个空格（「在 Ghostty 里」「30 秒」），与现有 `TerminalViews.swift` 的约定一致；书名号用「」；层级用「系统设置 › 通知」。
- 英文大小写：**窗格名、分组标题、按钮用 Title Case**（General、Allow Notifications）；**行标签、选项、说明用 sentence case**（Open at login、Icon and count）。原因：HIG 说 Title Case 偏正式、sentence case 偏随和，要求每类元素选一种并且贯彻到底 [3]；macOS 系统设置的行标签本身也多用 sentence case。按这条规则，现有代码里 Title Case 的行标签（如 "Who Tells You"）要统一改掉。
- 英文另写一版，不逐字对译：英文可以更短，用缩写形式（it's / don't）显得口语，比如 "Nothing's lost."。
- 数字：中文用阿拉伯数字加量词（「3 个会话」「30 秒」）；英文数字直接写成数字，时间写全称（"30 seconds"，不写 "30 s"）。快捷键用符号 ⌃⌥⌘⇧，不写 Ctrl。

### 2.6 错误与权限提示
- 错误三段式：**发生了什么 → 进度/数据是否安全 → 下一步按钮**。示例：「fix-invoice 断了 · 进度都在，点一下就找回。」按钮写动词，比如「找回」「打开系统设置」，不写「确定」。
- 权限两步：先由 cc-moss 自己用一句话讲清用处，再出现「去允许」按钮，点了才弹出系统对话框 [5]。权限没给时，也要让人知道现在能用什么替代办法（例：「允许之前先用 ⌃⌥M」）。
- 被拒绝时不指责、不反复弹：显示「在系统设置里关掉了」，再加一个「打开系统设置」按钮直接跳过去（HIG：给直达链接，不要描述位置 [3]）。
- 系统对话框里的说明字符串（purpose string）用主动语态的完整句，以句号结尾 [5]。

## 三、图标规范

### 3.1 样式
- 采用 macOS 系统设置的样式：彩色圆角方块，中间放白色 SF Symbol，单色渲染（monochrome）。
- 尺寸：方块 22×22 pt，圆角 6 pt（连续圆角 `.continuous`），符号 12 到 13 pt、`.semibold`、`.medium` scale，放在正中。这是参照系统设置得出的经验值，HIG 没有公开具体数字，需要在真机上对一下。
- 质感：从上到下有一层很浅的同色渐变（顶部亮约 8%），不加阴影、不描边；深色模式下亮度不变，保证白色符号的对比度。
- 一个窗格里所有符号用同一字重、同一 scale（HIG：符号字重和旁边文字的字重对应 [10]）。
- 选符号的原则：先选系统里已经有固定含义的（铃铛 = 通知，地球 = 语言，accessibility = 辅助功能）；用 `.fill` 变体，在小方块里更饱满；一个符号只表达一个意思，同一个符号不在两行重复用；不用带品牌或文字的符号；`.slash` 变体只用来表示不可用 [10]。
- 只用 macOS 14（SF Symbols 5）就有的符号，下表的符号都是 SF Symbols 1 到 4 引入的。

### 3.2 配色体系（一个分组一个色相）

| 分组 | 色相 | 说明 |
|---|---|---|
| 通用 | 系统蓝 `.blue` | 和系统设置的「通用」类保持一致 |
| 菜单栏 | 靛蓝 `.indigo` | 和通用区分开，同属冷色 |
| 小莫 | 品牌橙 `#D97757` | 小莫是唯一用品牌色的地方 [7] |
| 通知 | 系统红 `.red` | 和系统「通知」图标一致 |
| 关于 | 中性灰 `.gray` | 不常用，不抢眼 |
| 权限行 | 系统蓝 `.blue` | 和系统「辅助功能」图标一致 |

例外：通知窗格里的四种事件行沿用面板和菜单栏图标的状态色（等你 = 橙，断了 = 红，做完 = 绿，找回 = 青），让同一件事在各个地方颜色都一样。

### 3.3 每一项的图标、颜色和中英文案（可以直接替换）

格式：中文标签 / English label，下面一行是说明句。

**通用 General**（蓝）

| 原项 | SF Symbol | 色 | 中文 | English |
|---|---|---|---|---|
| 开机自动打开 | `power` | 蓝 | **开机自动打开**<br>开机后自己出来，不用你记着。 | **Open at login**<br>Starts with your Mac, so you don't have to remember. |
| 唤出面板（快捷键） | `keyboard.fill` | 蓝 | **唤出面板**<br>在哪儿按一下，都能看到所有会话。 | **Panel shortcut**<br>See every session from anywhere. |
| 界面语言 | `globe` | 蓝 | **语言**<br>跟随系统，也可以固定成中文或英文。 | **Language**<br>Follow your Mac, or pick one. |
| 新会话工作区 | `folder.fill` | 蓝 | **新会话从这里开始**<br>Claude 会在这个文件夹里干活。 | **Start new sessions in**<br>The folder Claude works in. |
| 会话在哪个终端打开 | `apple.terminal.fill` | 蓝 | **用哪个终端打开**<br>点开会话时，会在这里打开。 | **Open sessions in**<br>Where a session opens when you click it. |
| 试试 Ghostty | `sparkles` | 蓝 | **试试 Ghostty**<br>一个免费又好看的终端，想换可以试试。 | **Try Ghostty**<br>A free, good-looking terminal, if you fancy a change. |

**菜单栏 Menu Bar**（靛蓝）

| 原项 | SF Symbol | 色 | 中文 | English |
|---|---|---|---|---|
| 显示 | `menubar.rectangle` | 靛 | **菜单栏里显示**<br>数字先数在等你的，再数断了和在忙的。 | **Show in menu bar**<br>Counts what's waiting on you first, then the rest. |
| 　选项 | — | — | 只显示图标 / 图标加数字 | Icon only / Icon and count |
| 动态效果 | `wand.and.stars` | 靛 | **动态效果**<br>会话在忙时，图标会轻轻动。 | **Animation**<br>The icon moves gently while sessions work. |

**小莫 Mo**（品牌橙）

| 原项 | SF Symbol | 色 | 中文 | English |
|---|---|---|---|---|
| 住在桌面上 | `house.fill` | 橙 | **住在桌面上**<br>小莫待在屏幕角落，有事才出声。 | **Show Mo on desktop**<br>Mo sits in a corner and speaks up only when needed. |
| 大小 | `arrow.up.left.and.arrow.down.right` | 橙 | **大小**<br>挑一个看着顺眼的。 | **Size**<br>Whatever feels right. |
| 互动方式 | `face.smiling.inverse` | 橙 | **互动方式**<br>（说明随选项变，见下） | **How chatty**<br>(changes with the option) |
| 　安静 | — | — | **安静**：只做表情，你点它才说。 | **Quiet**: Just faces. Click Mo to hear more. |
| 　日常（默认） | — | — | **日常**：在等你或断了时，才开口说一句。 | **Everyday**: Speaks up when you're needed or something stops. |
| 　积极 | — | — | **积极**：每一轮做完也会来告诉你。 | **Lively**: Also tells you when each turn is done. |
| 出声方式 | `speaker.wave.2.fill` | 橙 | **出声方式**<br>不出声 / 轻响一声 / 念出来；30 秒最多一次。 | **Sound**<br>Silent / Chime / Speak. Once every 30 seconds at most. |
| 嗓音 | `waveform` | 橙 | **嗓音**<br>小莫说话时用的声音。 | **Voice**<br>The voice Mo speaks with. |
| 下载更好的声音 | `arrow.down.circle.fill` | 橙 | **下载更好的声音**<br>在系统设置里免费下载，听着自然得多。 | **Get better voices**<br>Free in System Settings, and far more natural. |

注：选了「不出声」时，「嗓音」和「下载更好的声音」两行隐藏（原则 7）。

**通知 Notifications**（红；事件行用状态色）

| 原项 | SF Symbol | 色 | 中文 | English |
|---|---|---|---|---|
| 谁来告诉你 | `bell.fill` | 红 | **谁来提醒你**<br>自动：小莫在就由它说，不在就发通知。 | **Who lets you know**<br>Automatic: Mo if it's around, otherwise a notification. |
| 　选项 | — | — | 自动 / 系统通知 / 小莫 | Automatic / Notifications / Mo |
| 通知中心权限 | `bell.badge.fill` | 红 | **系统通知**<br>打开后，点一下通知就能打开或找回会话。 | **Notifications**<br>Click one to open a session or bring it back. |
| 　状态值 | — | — | 已打开 / 在系统设置里关掉了 / 还没问过你 | On / Turned off in System Settings / Not asked yet |
| 需要你确认 | `hand.wave.fill` | 橙 | **在等你**<br>Claude 要你同意或回答问题时。 | **Waiting for you**<br>When Claude needs a yes or an answer. |
| 会话断了 | `bolt.slash.fill` | 红 | **会话断了**<br>意外停下时告诉你，进度都还在。 | **Session stopped**<br>When one stops unexpectedly. Nothing's lost. |
| 这一轮做完了 | `checkmark.circle.fill` | 绿 | **这一轮做完了**<br>每轮都会响，有点吵，所以默认关着。 | **Turn finished**<br>Comes after every reply, so it's off by default. |
| 找回完成 | `arrow.uturn.backward.circle.fill` | 青 | **找回来了**<br>断了的会话重新跑起来时。 | **Back again**<br>When a stopped session is running again. |

窗格底部的脚注（灰色小字，一句）：「同一个会话 30 秒内同样的事只提醒一次；面板开着时不弹。」/ "Repeats within 30 seconds count once. Nothing pops up while the panel is open."
诚实补充：「在等你」会比实际晚几秒才收到。这句话放在这一行的悬停提示里，不挤进说明句。

**关于 About**（灰）

| 原项 | SF Symbol | 色 | 中文 | English |
|---|---|---|---|---|
| 软件更新 | `arrow.triangle.2.circlepath` | 灰 | **软件更新**<br>有新版本时，在这里点一下就能更新。 | **Updates**<br>Get the newest version in one click. |
| 命令行程序位置 | `shippingbox.fill` | 灰 | **终端里的 cc-moss**<br>在终端里输入 cc-moss 也能用。 | **cc-moss in Terminal**<br>Type cc-moss in any terminal to use it there. |

注：完整路径只放在悬停提示和「在访达中显示」按钮里，不出现在说明句中。

**辅助功能权限行**（蓝，出现在「唤出面板」下方）

| 原项 | SF Symbol | 色 | 中文 | English |
|---|---|---|---|---|
| 辅助功能权限 | `accessibility` | 蓝 | **让 cc-moss 听见双击 ⌃**<br>在系统设置里打开一次就好，之前先用 ⌃⌥M。 | **Let cc-moss hear ⌃⌃**<br>Turn it on once in System Settings. Until then, use ⌃⌥M. |
| 　按钮 | — | — | 去允许 | Allow… |

可以再加一句悬停提示讲「为什么」：「双击 ⌃ 需要辅助功能权限，只用来认出这个手势。」⚠ 前提是代码确实只检查修饰键、不记录按键，确认之后才能用这句（原则 2）。

**别名 Aliases**：本次没有列入，按同一规则补。建议图标用 `tag.fill`，颜色用紫 `.purple`（新增一个分组色）。

## 四、可以借鉴的好产品做法

- **Raycast**：首次启动只让你做一件最关键的事，即设好全局快捷键，并主动帮你解决和 Spotlight 的快捷键冲突；跳过的步骤以后能在设置里找回来 [11]。对应到 cc-moss：欢迎页只设「唤出面板」快捷键，辅助功能权限放到用户第一次用双击 ⌃ 时再要。
- **Linear**：引导用的是任务清单，不是产品导览，可选步骤可以跳过，不放彩带、不庆祝 [12]。对应：欢迎页的三步可以跳过，完成后不放烟花。
- **Arc**：首次启动做成「开箱」体验，色彩很足，结尾给一张个性化的「会员卡」；新功能在用户用得上的时候才逐步展示 [13]。对应：欢迎页是小莫最该可爱的地方，设置页反而要收着。
- **Things**：界面大部分是中性色，颜色只用来表达含义，图标精致，留白多 [9][14]。对应：每个分组只用一种色相，状态色在全应用保持一致。

## 来源

1. Anthropic，Claude's Constitution（「brilliant friend」「diplomatically honest rather than dishonestly diplomatic」「warmth and care」「equanimity」等表述） — https://www.anthropic.com/constitution
2. Anthropic，Claude's Character — https://www.anthropic.com/research/claude-character
3. Apple HIG · Writing — https://developer.apple.com/design/human-interface-guidelines/writing
4. Apple HIG · Settings — https://developer.apple.com/design/human-interface-guidelines/settings
5. Apple HIG · Privacy（Requesting permission / purpose string） — https://developer.apple.com/design/human-interface-guidelines/privacy
6. Apple HIG · Onboarding — https://developer.apple.com/design/human-interface-guidelines/onboarding
7. rar.design，Claude interface design philosophy（陶土橙 #da7756、米白底、衬线体；二手分析） — https://rar.design/posts/claude-interface-design-philosophy
8. Abduzeedo，Anthropic branding by Geist（二手） — https://www.abduzeedo.com/seamlessly-crafting-ai-branding-and-visual-identity-anthropic
9. Blake Crosley，Things 3 design guide（二手） — https://blakecrosley.com/guides/design/things
10. Apple HIG · SF Symbols — https://developer.apple.com/design/human-interface-guidelines/sf-symbols
11. Raycast Manual · Getting Started — https://manual.raycast.com/windows/getting-started ；Raycast 设置说明 — https://raycastapp.notion.site/Settings-213d6e4a8215811a8fdfe5d83dace10f
12. Candu，Linear onboarding teardown — https://www.candu.ai/blog/linear-onboarding-teardown
13. Inverse，The Browser Company / Arc design interview — https://www.inverse.com/input/design/the-browser-company-arc-design-interview ；Onboard Me，Arc Max progressive disclosure — https://onboardme.substack.com/p/how-arc-browser-introduces-ai-max-feature
14. MacStories，Things 3: Beauty and Delight in a Task Manager — https://www.macstories.net/reviews/things-3-beauty-and-delight-in-a-task-manager/
15. Apple HIG · The menu bar（menu bar extras：用单色符号，让用户自己决定是否放进菜单栏） — https://developer.apple.com/design/human-interface-guidelines/the-menu-bar

说明：[1][3][4][5][6][10][15] 是一手资料，Apple HIG 是通过官方 JSON 接口读取的原文。Anthropic 的视觉品牌目前没有公开的官方规范，[7][8] 是二手分析，只用来参考配色方向。
