# Slack App 通道 + macOS 在场检测：事实清单

> 调研日期 2026-10-18。每条事实附来源 URL。标 **【未验证】** 的条目来自第三方或推断，没有在官方文档里查到原文；标 **【本机实测】** 的条目在一台 macOS 26.6.2 机器上跑过（只读，没改系统）。
> 本文按公开仓库规则写：不出现任何工作区名、令牌、主机名、账号名。

---

## (a) Slack

### a1. Socket Mode（不需要公网 URL）

- Socket Mode 通过 WebSocket 接收 Events API 和交互 payload，不需要公网 HTTP Request URL。WebSocket URL 在运行时调用 `apps.connections.open` 生成，并且会定期刷新。来源：https://docs.slack.dev/apis/events-api/using-socket-mode
- `apps.connections.open` 用 **app-level token（`xapp-` 开头）**，放在 `Authorization: Bearer` 头里；放进 POST 参数会报错。方法本身"No scopes required"；速率 Tier 3（50+/分钟）。来源：https://docs.slack.dev/reference/methods/apps.connections.open
- 创建 app-level token 时要勾 `connections:write` scope，这是 Socket Mode 的惯例；上面的方法页写的是方法本身不需要 scope。**【未验证：本次没在官方页面抓到 `connections:write` 的原文，但 SDK 和社区一致这么用】** 来源：https://docs.slack.dev/apis/events-api/using-socket-mode
- 调 Web API（发消息、开 modal 等）用 **bot token（`xoxb-`）**，在安装到工作区时得到；它和 app-level token 是两把不同的钥匙，两把都要存。来源：https://docs.slack.dev/authentication/best-practices-for-security
- 连上后 Slack 先发 `{"type":"hello","num_connections":1,"approximate_connection_time":3600,...}`；`approximate_connection_time` 是连接预计还能活多少秒。来源：https://docs.slack.dev/apis/events-api/using-socket-mode
- 断开消息 `type:"disconnect"` 的 `reason` 有三种：`link_disabled`（Socket Mode 被关了，不该重连）、`warning`（大约 10 秒后断）、`refresh_requested`（要求刷新）。原文："you'll need to handle connection refreshes once every few hours"。来源：同上
- 调试时可以在 wss URL 后面加 `&debug_reconnects=true`，连接寿命缩到 360 秒，方便测重连。来源：同上
- 一个 app 最多同时开 **10 条** WebSocket 连接，每个 payload 会投给其中**任意一条**。所以多台 Mac 共用同一个 App 时，事件不会广播给每台机器。来源：同上
- 每个事件都要 ack，Slack 据此决定要不要重投。来源：同上
- 限流：Web API 返回 HTTP 429，带 `Retry-After` 头（秒）；Events API 投递上限是每个工作区、每个 app、每 60 分钟 30,000 次，超了会收到 `app_rate_limited` 事件。来源：https://docs.slack.dev/apis/web-api/rate-limits
- 2025-05-29 起，新建的、未上 Marketplace 的**商业分发** app 调 `conversations.history` / `conversations.replies` 会受新限制；**internal app 不受影响**。来源：同上
- **分发限制**："Apps using Socket Mode are not currently allowed in the public Slack Marketplace"；替代方案是 Enterprise 的 org-wide deployment。来源：https://docs.slack.dev/apis/events-api/using-socket-mode
- 2024-12 开发者政策：大规模商业分发要走 Marketplace 审核，并且禁止用 unlisted 或自定义 app 绕开 Marketplace 规则。来源：https://api.slack.com/changelog/2024-12-dev-policy-update
- 推论：每个用户用 manifest 在自己的工作区建一个**自用（单工作区）**的 app，属于 internal app 用法，不受上面的 Marketplace 限制和 history 限流影响。**【推论，未经 Slack 书面确认】**

### a2. 双向消息

- `message.im` 事件需要 `im:history` scope，DM 用 `event.channel_type == "im"` 区分。来源：https://docs.slack.dev/reference/events/message.im
- App Home 的 Messages tab 是用户和 app 的 1:1 对话；收用户消息靠订阅 `message.im`，回复用 `chat.postMessage`。来源：https://docs.slack.dev/surfaces/app-home
- `app_mention` 用于在频道里 @bot（需要 `app_mentions:read`）。**【本次没抓官方页原文】** 来源：https://docs.slack.dev/reference/events/app_mention
- `chat.postMessage` 需要 `chat:write`。`channel` 填用户 ID 时会自动打开和 bot 的 DM；bot 不能发进别的两个人之间的 DM；没有权限时报 `channel_not_found`。来源：https://docs.slack.dev/reference/methods/chat.postMessage
- 线程：`thread_ts` 填**父消息**的 `ts`，不要填回复的 `ts`；`reply_broadcast` 会同时发到频道，官方建议少用。来源：同上
- 文本长度：`text` 建议不超过 4,000 字符，超过 40,000 会被截断；`markdown_text` 上限 12,000。来源：同上
- 速率：`chat.postMessage` 大约每频道每秒 1 条，另有工作区级上限，允许短时突发。来源：https://docs.slack.dev/apis/web-api/rate-limits
- 消息里的 block 数上限（常说的 50 个）这次没在官方页面里找到原文。**【未验证】**
- `chat.update` 需要 `chat:write`，Tier 3；只能改自己发的消息；**ephemeral 消息改不了**。来源：https://docs.slack.dev/reference/methods/chat.update
- `chat.postEphemeral` 必须带 `channel` 和 `user`，只有这一个人看得见，对方刷新或离开频道后就没了。来源：https://docs.slack.dev/messaging/sending-and-scheduling-messages
- Home tab 用 `views.publish(user_id, view{type:"home"})` 发布，每次都是整页替换，最多 100 个 block；`app_home_opened` 事件里带 user ID。来源：https://docs.slack.dev/surfaces/app-home

### a3. 交互式选择（Block Kit）

- 交互 payload 要在 **3 秒内** ack，否则用户会看到报错；`response_url` 可以在 30 分钟内用最多 5 次。来源：https://docs.slack.dev/interactivity/handling-user-interaction
- `trigger_id` 3 秒后过期，只能用一次（报 `trigger_expired` / `trigger_exchanged`）；`views.open` 必须用有效的 `trigger_id`。modal 的 view stack 最多 3 层；`view_submission` 的 `response_action` 可以是 `update`/`push`/`errors`/`clear`。来源：https://docs.slack.dev/surfaces/modals
- Socket Mode 下交互 payload 的信封是 `type:"interactive"`，`accepts_response_payload:true`；可以在 ack 里带 payload，比如 `{"envelope_id":..., "payload":{"response_action":"update",...}}`。来源：https://docs.slack.dev/apis/events-api/using-socket-mode
- 3 秒 ack 时限在 Socket Mode 下同样适用。**【推论：Socket Mode 页没写具体秒数，交互文档的 3 秒是按 HTTP 写的】**
- actions block 最多放 25 个元素；`block_id` 最长 255；更新消息时建议换新的 `block_id`。来源：https://docs.slack.dev/reference/block-kit/blocks/actions-block
- `checkboxes` 最多 10 个选项，可用于消息、modal 和 Home。来源：https://docs.slack.dev/reference/block-kit/block-elements/checkboxes-element
- `multi_static_select` 最多 100 个选项，每个选项文本少于 76 字符，`max_selected_items` 最小为 1，可用于消息、modal 和 Home。来源：https://docs.slack.dev/reference/block-kit/block-elements/multi-select-menu-element
- `input` block 可以放在**消息**、modal 和 Home 里；`dispatch_action:true` 时输入会触发 `block_actions`；文本框用 `plain_text_input`。来源：https://docs.slack.dev/reference/block-kit/blocks/input-block
- 把"N 选项单选/多选题 + 自由文本"映射成 Block Kit 的建议（**设计推论**）：
  - 单选且 N ≤ 5：消息里用 `section` 写题干，`actions` 放 N 个 button（`value` 填选项序号）。点一下就回答，最省事。
  - 单选且 N 较多：用 `static_select`，上限 100。
  - 多选且 N ≤ 10：用 `checkboxes` 加一个"提交"button。N 更多时用 `multi_static_select`。
  - 自由文本：消息里用 `input`+`plain_text_input`（`dispatch_action` 或提交按钮）；也可以放一个"其他…"button，用它的 `trigger_id` 调 `views.open` 开 modal。modal 的体验更稳，可以用 `errors` 校验。
  - 也可以直接让用户在线程里回复文字当作自由文本答案，零 Block Kit。
  - 回答后用 `chat.update` 把按钮换成"已选：X"，防止重复点击。

### a4. 人设（Persona）

- app 名称在 manifest 的 `display_information.name` 设置（最长 35 字符）；bot 显示名在 `features.bot_user.display_name` 设置，`always_online` 控制是否一直显示在线。来源：https://docs.slack.dev/reference/app-manifest
- 图标：manifest 的 JSON/YAML 字段表里没有 icon 字段（只有 TypeScript 的 `Manifest()` 示例里有 `icon`）。从 URL 创建 app 时图标要到 app 设置页手动上传。**【未验证：没有找到可以通过 API 上传图标的官方说明】** 来源：同上
- 单条消息覆盖：`chat.postMessage` 的 `username`、`icon_emoji`、`icon_url` 需要 `chat:write.customize`（同时要有 `chat:write`），只适用于 bot token。来源：https://docs.slack.dev/reference/scopes/chat.write.customize 、https://docs.slack.dev/reference/methods/chat.postMessage
- 使用约束：只能在用户本人同意、并且由用户操作触发时使用（"must only be used when the user themselves gives permission"）。来源：https://docs.slack.dev/messaging/sending-and-scheduling-messages
- 自定义名字和头像后，消息仍然算 bot 消息，名字旁边的 "APP" 标签依旧会显示，去不掉；只有走用户 token 才能以真人身份发。**【未验证：来自 bolt-js issue 讨论，不是官方文档原文】** 来源：https://github.com/slackapi/bolt-js/issues/2092
- 结论：一个 app 可以用自定义人设名发消息（manifest 的 name 和 display_name 由用户自己定，或者每条消息覆盖），但一定会被标成 app。

### a5. 给非开发者的安装流程

- manifest 支持 YAML 和 JSON。预填创建链接：`https://api.slack.com/apps?new_app=1&manifest_json=<URL编码的JSON>`（也有 `manifest_yaml=`），内容必须 URL 编码。来源：https://docs.slack.dev/app-manifests/configuring-apps-with-app-manifests
- 要写进 manifest 的字段：`settings.socket_mode_enabled: true`、`settings.interactivity.is_enabled: true`、`settings.event_subscriptions.bot_events: [message.im, app_home_opened, ...]`、`features.app_home.messages_tab_enabled: true`、`messages_tab_read_only_enabled: false`、`oauth_config.scopes.bot: [...]`、`settings.token_rotation_enabled: false`（打开轮换后要处理 refresh，桌面端会麻烦很多）。来源：https://docs.slack.dev/reference/app-manifest
- 最小 bot scope（**设计推论**）：`chat:write`、`im:history`（收 DM）。可选：`im:write`（主动开 DM；`chat.postMessage` 填用户 ID 一般就够）、`app_mentions:read`、`chat:write.customize`、`users:read`（显示名）。
- app-level token **没法通过 manifest 生成**，必须让用户到 Basic Information → App-Level Tokens 手动生成（勾 `connections:write`），再复制回桌面 app。**【未验证：manifest 文档里没提，也没找到能生成它的 API】**
- 这样用户要做：① 打开预填链接，选工作区，点 Create；② Install to Workspace，复制 `xoxb-`；③ 生成 `xapp-` 并复制；④ 粘贴进桌面 app。桌面 app 拿到 token 后可以调 `auth.test` 校验 token，并拿到 bot 的 user_id 和 team_id。
- 如果工作区开了 app 审批，普通成员装不了没审批的 app，要管理员来装。**【第三方来源】** 来源：https://help.cloudiway.com/article/slack-app-installation-error-this-app-isnt-listed-in-the-slack-marketplace/
- 另一条路是 App Manifest API（`apps.manifest.create`），但它要用 config token。config token 按"用户 × 工作区"发，不按 app 发，对非开发者并不比网页流程简单。来源：https://docs.slack.dev/app-manifests/configuring-apps-with-app-manifests

### a6. Token 存储与安全

- 官方建议：不要硬编码，不要明文写盘，桌面端用系统 Keychain 或其他加密机制；不要在 UI 或日志里显示 token；只申请最少的 scope；分发的原生 app 里不要带 client secret。来源：https://docs.slack.dev/authentication/best-practices-for-security
- macOS 上用 Keychain 的 generic password（`kSecClassGenericPassword`）存 token，配上 service 和 account 键，`kSecAttrAccessibleAfterFirstUnlock` 或 `WhenUnlocked`。**【常规做法，本次没单独引用 Apple 页】** 参考：https://developer.apple.com/documentation/security/keychain-services
- 可以给 token 配 IP 白名单（不支持 IPv6，对 incoming webhook 无效）。家用宽带 IP 会变，不推荐用这个。来源：https://docs.slack.dev/authentication/best-practices-for-security
- 谁能给 bot 发 DM：工作区里的任何成员都能打开 app 的 Messages tab 发消息，Slack 本身不提供"只允许安装者"的开关。**【未验证为官方表述，按产品行为推断】** 所以必须在桌面端做白名单：只处理 `event.user ∈ allowed_user_ids` 的消息，默认只放行安装者本人（引导时用 `auth.test` 或第一次 DM 绑定）；交互 payload 也要校验 `user.id`。
- 要丢弃 `bot_id` 不为空或带 `subtype` 的消息事件，避免回声循环。**【常规做法】**

### a7. 轻量客户端与最小协议

- 官方 SDK（Bolt）有 JS、Python、Java。Rust 有社区库 `slack-morphism`（2.24.x，支持 Web API、Events、Socket Mode、Block Kit，自带 `socket_mode` 示例）。来源：https://docs.rs/crate/slack-morphism/2.24.1
- Swift 没有官方 SDK；社区的 SlackKit 等库维护状态存疑。**【未验证】**
- 自己用原生 WebSocket（Rust 用 tokio-tungstenite，Swift 用 URLSessionWebSocketTask）加 HTTPS Web API 完全可行，协议很小（**依据上面 Socket Mode 文档整理**）：
  1. `POST https://slack.com/api/apps.connections.open`，头里带 `Authorization: Bearer xapp-…`，返回 `{ok:true, url:"wss://…"}`。
  2. 连接这个 wss，收到 `hello`，记下 `approximate_connection_time`。
  3. 收到 `{envelope_id, type: events_api|interactive|slash_commands, payload, accepts_response_payload}`，**先**回 `{"envelope_id":"…"}`（必要时带 `payload`），再异步处理业务。
  4. 收到 `disconnect`：`warning` 或 `refresh_requested` 时先调 `apps.connections.open` 开一条新连接，再关旧连接；`link_disabled` 时停止并提示用户。
  5. 网络断开或 ping 超时：指数退避重新走第 1 步；同一个 payload 可能被重投，按 `event_id` 或 `envelope_id` 去重。
  6. Web API 统一用 `POST https://slack.com/api/<method>`，头里带 `Bearer xoxb-…`，body 是 JSON；遇到 429 按 `Retry-After` 等待。

---

## (b) macOS 在场检测

### b1. 锁屏

- `CGSessionCopyCurrentDictionary()` 返回当前 window server 会话的字典；如果进程不在 GUI 会话里（比如 SSH、launchd 的 system 域）就返回 NULL。来源：https://developer.apple.com/documentation/coregraphics/cgsessioncopycurrentdictionary()
- 官方只公开了 `kCGSessionUserIDKey`、`kCGSessionUserNameKey`、`kCGSessionConsoleSetKey`、`kCGSessionOnConsoleKey`、`kCGSessionLoginDoneKey` 这几个键；**`CGSSessionScreenIsLocked` 是私有键，没有文档**。来源：https://developer.apple.com/documentation/coregraphics/window-server-session-properties
- 社区做法是在锁屏时检查字典里有没有 `CGSSessionScreenIsLocked`（值为 1）。来源：https://community.jamf.com/t5/jamf-pro/run-policy-only-when-the-user-session-is-not-locked/m-p/167660/highlight/true
- **【本机实测】** macOS 26.6.2 未锁屏时，字典里有 `kCGSSessionOnConsoleKey=1`、`kCGSessionLoginDoneKey=1`、`kCGSSessionAuditIDKey` 等，**没有** `CGSSessionScreenIsLocked`。锁屏时这个键会不会出现，本次没测（没法在无人值守时锁屏）。**【macOS 14/15/26 上锁屏时的行为未验证】**
- 分布式通知 `com.apple.screenIsLocked` / `com.apple.screenIsUnlocked` 被 Chromium 的 idle 检测等大量代码使用，但 **Apple 没有文档**。来源：https://chromium.googlesource.com/chromium/src/+/e8a107c96/ui/base/idle/idle_mac.mm
- 沙盒：Mojave 起，沙盒 app 不能用 `name:nil` 订阅全部分布式通知，但仍然可以按具体名字订阅（这个行为曾被当作信息泄露报告）。在 14/15/26 的沙盒下按名字订阅还能不能用，**【未验证】**。来源：https://www.jamf.com/blog/0day-mojaves-sandbox-is-leaky/
- 稳妥做法：通知（事件驱动）加启动时和定时轮询 `CGSessionCopyCurrentDictionary`（兜底），两路互相校正。

### b2. 空闲时长

- `CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: kCGAnyInputEventType)` 返回距离上次键盘、鼠标或数位板输入过了多少秒，文档没提到任何权限要求。来源：https://developer.apple.com/documentation/coregraphics/cgeventsource/secondssincelasteventtype(_:eventtype:)
- **【本机实测】** 在非沙盒的命令行 Swift 里，`CGEventType(rawValue: ~0)` 当作 `kCGAnyInputEventType` 可以用，没有弹任何 TCC 授权（不需要辅助功能或输入监控权限），`.combinedSessionState` 和 `.hidSystemState` 返回的值几乎一样。
- IOKit 的 `IOHIDSystem` 的 `HIDIdleTime` 属性（单位纳秒）是另一个来源，`ioreg -c IOHIDSystem` 能读到。**【本机实测，Apple 没有专门的文档页】** 参考：https://community.jamf.com/general-discussions-2/check-if-screen-is-locked-in-a-script-872
- 注意：看视频、开会时没有输入也算空闲，所以空闲只能当"可能离开"的信号，最好和电源断言（`pmset -g assertions` 里的 PreventUserIdleDisplaySleep）一起判断。**【设计推论】**

### b3. 显示器睡眠、系统睡眠、屏保、快速用户切换

- `NSWorkspace.screensDidSleepNotification` / `screensDidWakeNotification` 在屏幕睡眠和唤醒时发出；`willSleepNotification` / `didWakeNotification` 对应系统睡眠。必须注册在 `NSWorkspace.shared.notificationCenter`，注册到别的 center 收不到。来源：https://developer.apple.com/documentation/appkit/nsworkspace/screensdidsleepnotification
- `NSWorkspace.sessionDidResignActiveNotification` / `sessionDidBecomeActiveNotification` 在快速用户切换、会话切出和切回时发出；如果 app 在不活跃的会话里启动，会在 `didFinishLaunching` 之前先收到 resign。来源：https://developer.apple.com/documentation/appkit/nsworkspace/sessiondidresignactivenotification
- `kCGSessionOnConsoleKey`（公开键）也能用来判断会话是不是在前台控制台上。来源：https://developer.apple.com/documentation/coregraphics/window-server-session-properties
- 屏保：分布式通知 `com.apple.screensaver.didstart` / `didstop`（也有 `willstop`），Chromium 用它设置"屏保运行中"标志；没有 Apple 文档。一个老帖子说要求密码时，弹出认证框就会触发 didstop。**【未验证：在现代 macOS 上的行为】** 来源：https://chromium.googlesource.com/chromium/src/+/e8a107c96/ui/base/idle/idle_mac.mm 、https://lists.apple.com/archives/Cocoa-dev/2005/Aug/msg01849.html
- 系统睡眠期间 WebSocket 一定会断，唤醒后要主动重连 Slack（见 a7 第 5 步）。**【设计推论】**

### b4. 推荐的在场状态机

- 参考阈值：Slack 桌面端约 **10 分钟**没有活动就自动设为 away。来源：https://slack.com/help/articles/201864558-Set-your-Slack-status-and-availability
- Teams 约 **5 分钟**没有键鼠活动设为 Away，锁屏立刻 Away，睡眠显示 Offline。**【第三方来源，Microsoft 官方近期措辞是"a few minutes"】** 来源：https://help.vccs.edu/TDClient/3184/BRCCPortal/KB/ArticleDet?ID=158104 、https://www.virtosoftware.com/pm/microsoft-teams-status/
- 状态（**设计推论**）：
  - `present`：有输入，idle < 2 分钟。
  - `away_short`：2 分钟 ≤ idle < 10 分钟，没锁屏（可能在看屏幕或者短暂离开）。
  - `away`：idle ≥ 10 分钟，或屏幕睡眠，或屏保运行中。
  - `locked`：收到 screenIsLocked、会话失活（快速用户切换），或系统睡眠。优先级最高，立刻进入。
- 迟滞：从 away 或 locked 回到 present 要满足"解锁"或"连续 ≥ 3–5 秒有输入"（防止碰一下鼠标就误判）；从 present 进入 away 用 idle 阈值，不要立刻切；状态变化后 30–60 秒内不重复发"切到 Slack 通道"的通知。轮询 idle 的间隔用 5–15 秒就够。

---

## 对设计的影响

1. **每个用户自建、自用的 Socket Mode app 是唯一合规的轻量路线。** Socket Mode 不能上 Marketplace，我们不发公共 app，而是分发一份 manifest 加预填链接，每个人在自己的工作区建一个 internal app。
2. **引导流程分 4 步，第 3 步要手动操作**：打开预填 manifest 链接 → Install 拿 `xoxb-` → 手动生成 `xapp-`（勾 connections:write）→ 粘贴。UI 里要给截图或指引，粘贴后立刻调 `auth.test` 校验。
3. **两把 token 都存进 Keychain**，不写配置文件，不进日志；关掉 token 轮换，降低桌面端复杂度。
4. **Socket Mode 客户端自己写**（Swift URLSessionWebSocketTask 或 Rust tungstenite），协议就是 open → hello → 先 ack 再处理 → disconnect 时先开新连接再关旧连接，加指数退避和去重；不引入 Bolt 运行时。
5. **多台 Mac 共用一个 App 时事件只投给一条连接**：每台 Mac 要么各用一个 App，要么设计"主机认领"机制。最简单的做法是一台 Mac 一个 App 或人设。
6. **一定要做白名单**：只处理 `allowed_user_ids`（默认是安装者）发的 DM 和交互，过滤 bot_id 和 subtype；任何工作区成员都能给 bot 发 DM。
7. **提问映射**：N ≤ 5 单选用按钮，多选 ≤ 10 用 checkboxes 加提交，更多用 select 或 multi-select；自由文本优先用线程回复或 modal。回答后用 `chat.update` 锁定答案；ack 必须在 3 秒内，业务异步处理。
8. **人设**：名字和显示名写进 manifest（用户可以改），图标要手动上传；需要多个人设时用 `chat:write.customize` 覆盖 username 和 icon。"APP" 标签一定会显示，不要承诺"像真人"。
9. **长输出要分段**：单条 text 不超过 4,000 字符，发送节奏不超过每频道每秒 1 条；Home tab 适合放"当前会话状态"面板（整页替换，最多 100 个 block）。
10. **在场检测分层组合**：事件层（screenIsLocked/Unlocked、screensDidSleep/Wake、willSleep/didWake、sessionDidResign/BecomeActive、screensaver）加轮询层（idle 秒数、CGSession 字典），全部不需要 TCC 权限；私有键和无文档的通知要有兜底，并在 14/15/26 上做实机锁屏验收。
11. **默认阈值**：present < 2 分钟，away ≥ 10 分钟（和 Slack 一致），锁屏或睡眠立即进 locked；回到 present 需要解锁或持续输入，避免在 Mac 和 Slack 之间来回推送。
12. **睡眠和唤醒是 Slack 连接的生命周期事件**：睡眠前可以主动断开，唤醒后立即重连并补发离线期间积压的提问，同时把在场状态复位到 present 候选。
