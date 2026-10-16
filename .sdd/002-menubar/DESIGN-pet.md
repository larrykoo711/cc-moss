# DESIGN — 桌面宠物（002 P3，T3.1）

状态：**待用户批准（G1）**。本档是 `DESIGN-menubar.md` §1.1 里「桌面宠物」一行的展开，编号 P1–P30 只在本档内使用；与 A1–A22 冲突时以 A 系列为准。实现分在 T3.2（窗口与动画）、T3.3（气泡）、T3.4（语音）；004（代办确认）、005（语音对话）只留接口。

## 1. 一句话

桌面角落里住着一个小家伙「小莫」（Mo）：会话在忙它就转着眼睛想事情，需要你时它跳起来招手，会话断了它皱眉，找回来了它眯眼笑；它只在值得打扰的时候开口，开口也只说一句短话。它是同一条事件流（A13）的又一种演出方式，不另起数据源。

设计取向（MOSS 的气质）：**安静、可靠、偶尔幽默**。默认多数时间一动不动地陪着；动作小、节奏慢；话少，说的都是有用的。

## 2. 调研结论

### 2.1 窗口技术（结论：确定可行）

- 透明、无边框、不抢焦点的常驻小窗，macOS 的标准做法是 `NSPanel` + `.nonactivatingPanel`：普通 `NSWindow` 显示时会激活本 App，甚至把全屏 App 踢出它的空间，非激活面板不会（[Apple 开发者论坛：跨空间置顶的覆盖窗口](https://origin-devforums.apple.com/forums/thread/826308)、[fazm：SwiftUI 浮动面板的 NSPanel 做法](https://fazm.ai/blog/swiftui-floating-panel)）
- `.canJoinAllSpaces` 让面板跟着用户换桌面；`.fullScreenAuxiliary` 让它出现在全屏 App 之上。**我们反过来利用这一点：不加 `.fullScreenAuxiliary`，原生全屏时宠物自然不出现**（同上两处）
- 透明背景 `backgroundColor = .clear`、`isOpaque = false`，内容用 `NSHostingView` / 图层承载；`becomesKeyOnlyIfNeeded = true`，宠物永不抢键盘（[macOS patterns：浮动面板](https://skills.cat/skills/fayazara/macos-app-skills/macos-patterns)、[levelup：SwiftUI 浮动窗口](https://levelup.gitconnected.com/swiftui-macos-floating-window-panel-4eef94a20647)）
- 同类开源实现都走这条路：Swift 写的桌面宠物 sakana（可切换置顶与普通层级、弹簧物理摇摆、拖动交互，macOS 14+，MIT）、neko（住在屏幕边缘、按昼夜睡觉）、mochi（菜单栏附近的像素小伙伴）、desktop-pet（播放 GIF / APNG / 视频）（[sakana](https://github.com/fimars/sakana)、[neko](https://awesome.ecosyste.ms/projects/github.com%2Fsuperbereza%2Fneko)、[mochi](https://awesome.ecosyste.ms/projects/github.com%2Fzekiahmetbayar%2Fmochi)、[desktop-pet](https://tomevault.io/tome/bssm-oss/desktop-pet)）。我们只参考它们的窗口做法，不使用其任何形象或素材
- 检测「别的 App 在全屏 / 演示」：没有公开 API 拿别的进程的窗口样式；Apple DTS 建议用 `CGWindowListCopyWindowInfo` 的在屏窗口 + 窗口边界与显示器边界比对做近似，WebRTC 的窗口工具也是这么判定的；要注意刘海屏、自动隐藏菜单栏、常驻 Dock 的误判，并按窗口所属进程过滤（[论坛：全屏检测](https://developer.apple.com/forums/thread/792917)、[论坛：用 Core Graphics 检测全屏](https://developer.apple.com/forums/thread/779272)、[WebRTC window_list_utils](https://webrtc.googlesource.com/src.git/+/3e70781361ed/modules/desktop_capture/mac/window_list_utils.cc)）。取窗口边界不需要屏幕录制权限，取窗口标题才需要，我们只用边界

### 2.2 动画形式（结论：Core Animation 驱动自绘几何，零依赖）

| 方式 | 依赖与体积 | 运行开销 | 表现力 | 结论 |
|------|-----------|----------|--------|------|
| SwiftUI `Shape` + `TimelineView` 逐帧重算 | 无 | 每帧在本进程里重新布局；有菜单栏 App 连续动画占 20–30% CPU 的报告（[论坛](https://developer.apple.com/forums/thread/767243)），macOS 上 SwiftUI 不可见时也可能持续重算（[mjtsai 2023](https://mjtsai.com/blog/2023/10/04/control-center-in-macos-always-using-cpu)） | 高 | 只用于静态内容与短过渡（气泡） |
| **Core Animation（`CAShapeLayer` + 关键帧 / 弹簧动画）** | 无（系统框架） | 动画在渲染服务器里跑，本进程空闲；Lottie 4 正是为此把渲染搬到 Core Animation（旧的主线程引擎播放时占 5–20% CPU）（[InfoQ：Lottie 4](https://www.infoq.com/news/2022/12/airbnb-lottie-4/)、[Rive 文档：Apple 端资源占用](https://rive.app/docs/runtimes/apple/resource-usage)） | 足够：路径形变、描边截取、位移缩放、弹簧 | **采用** |
| 帧图（PNG 序列 / GIF / APNG） | 素材体积随帧数与尺寸涨；要画师 | 解码与贴图便宜 | 取决于画工；改一个细节要重出全部帧 | 不采用：与「参数化、可调大小、深浅色」冲突 |
| Lottie | SPM 包约 500 KB 源码，xcframework 约 10 MB（[lottie-ios 4.1 发布说明](https://newreleases.io/project/yarn/lottie-ios/release/4.1.0)） | Core Animation 引擎，低 | 高，需 After Effects 工作流 | 不采用：新依赖 + 设计工具链，收益不抵 |
| Rive | 原生库在 Android 上实测约 12 MB（[Wikimedia 评估](https://phabricator.wikimedia.org/T433250)）；Apple 端自带 Metal 渲染 | Metal，本进程可见 | 最高（状态机） | 不采用：依赖最重；我们的动作简单到不需要状态机编辑器 |

关键点：**所有动作都由同一条 Continuum 曲线（x = 32 + 27 cos t，y = 32 + 12.9 sin 2t）改几个参数得到**——上下振幅、左右瓣缩放、眼角下垂、缺口位置与长度。实现时每个动作的路径点数相同，Core Animation 能在任意两个动作之间直接做路径形变，不需要中间帧。

### 2.3 打扰控制（结论：默认保守，宁可少说）

- Clippy 的教训：心理学顾问事后说它最糟的一点就是「它会打断人」；斯坦福的论文结论是助手要让人「觉得自己说了算」、遵守人与人之间的礼仪（[哈佛 Crimson 2005](https://www.thecrimson.com/article/2005/4/28/what-not-to-do-with-a/)）。2026 年一项游戏内助手的研究同样发现，玩家反感的很少是内容，而是时机——对每个动作都回应的助手像「打断你干活的不速之客」（[arXiv 2609.13718](https://arxiv.org/pdf/2609.13718)）
- Apple 的通知分级：被动（随时看）、主动（默认）、时效（直接影响你、需要马上处理，才可越过专注模式）、紧急（健康安全）（[HIG：管理通知](https://developer-rno.apple.com/design/human-interface-guidelines/patterns/managing-notifications)、[WWDC21 10091](https://developer.apple.com/videos/play/wwdc2021/10091?time=117)）。我们把宠物的话也分成这几级（§6），只有「需要你」接近「时效」级
- 系统在镜像或共享屏幕时默认不显示通知，这是用户在「系统设置 → 通知」里可改的选项（[Apple 支持：通知设置](https://support.apple.com/en-za/guide/mac-help/mchl205da693)）。宠物同样默认在演示、共享时退场
- 专注模式：`INFocusStatusCenter` 可读「是否在专注」，需要授权，有开发者报告只能轮询、KVO 不生效，macOS 上是否可用存疑（[论坛：Focus Status API](https://developer.apple.com/forums/thread/682143)、[INFocusStatus 文档](https://developer.apple.com/documentation/intents/infocusstatus.md)）。T3.3 先做探针，不行就只用宠物自己的勿扰

### 2.4 本机语音（结论：`AVSpeechSynthesizer`，只读短句，默认关）

- 系统自带合成，按 `quality` 分默认 / 增强 / 高级三档；增强与高级要用户在「系统设置 → 辅助功能 → 朗读内容」里下载，每个 100 MB 以上，用户也可能删掉，所以每次说话前要确认选中的声音还在（[Ben Dodson：在 App 里用 Personal Voice](https://bendodson.com/weblog/2024/04/03/using-your-personal-voice-in-an-ios-app/)、[AVSpeechSynthesisVoice 文档](https://developer.apple.com/documentation/avfaudio/avspeechsynthesisvoice.md)）
- 没有 API 列出「可下载但未下载」的声音，只能列已装的（[论坛 758460](https://developer.apple.com/forums/thread/758460)）；所以设置里列已装声音，并给一个「去系统设置下载更好的声音」的入口
- 中文高级声音的标识形如 `com.apple.voice.premium.zh-CN.<名字>`（社区资料，[mac-tts](https://github.com/rrlab-tech/mac-tts)），以本机 `speechVoices()` 为准，不写死
- 沙盒 App 里增强声音可能无声是已知问题（[论坛 844422](https://developer.apple.com/forums/thread/844422)）；cc-moss 不开沙盒（A12 的 ad-hoc 分发），不受影响，但记为风险

## 3. 形象：三个方向

全部由 Continuum 几何自绘，加上 SF Symbols 的 `sparkle`、`zzz`；不使用、不参照任何第三方角色或吉祥物。原型图：`diagrams/pet/directions-zh-light.png`、`directions-en-dark.png`（`CCMoss --pet-sheets <目录>` 生成，代码 `apps/macos/Sources/CCMoss/PetPrototype.swift`）。

| 方向 | 长相 | 长处 | 短处 |
|------|------|------|------|
| A · 环 | 标志本身活过来：一条粗的品牌色无限环漂在桌面上，靠形变、缺口流动、颜色表达状态 | 品牌最纯粹，最小最安静 | 没有脸，「担心」「开心」这种情绪难读；静止时像一个贴纸而不是伙伴 |
| **B · 小莫（推荐）** | 一颗石墨色圆角小石头（与 App 图标同一种底色），无限环是它发光的双眼，头顶一根小天线，天线灯是状态灯 | 64 pt 也一眼可读；眼睛形变就能表达六种情绪；天线灯在勿扰时还能无声提醒；与 App 图标是一家人 | 比 A 多一个「身体」，需要控制体积别抢戏 |
| C · 轨道 | 一颗蓝色小星球，无限环是它的轨道，一颗品牌色卫星沿轨道跑 | 最有电影感（流浪的地球） | 品牌色只剩一个小点；四个元素（星球、眼、轨道、卫星）在小尺寸下拥挤；蓝色与品牌不统一 |

推荐 B 的理由：它把「无限环 = 延续」放在了最有表情的位置（眼睛），情绪表达几乎全靠一条曲线的参数，正好契合零依赖的参数化动画；石墨色身体在浅色和深色桌面上都有足够对比（见两套原型图）。

名字：中文「小莫」，英文「Mo」（取自 MOSS，短、好叫）。待用户拍板（§13）。

## 4. 动作表（小莫）

图：`diagrams/pet/poses-zh-light.png`、`poses-en-dark.png`（六个动作），`frames-zh-light.png`、`frames-en-dark.png`（在忙、眨眼、需要你三段关键帧）。

| 动作 | 何时 | 外观参数 | 时长与循环 | 天线灯 |
|------|------|----------|------------|--------|
| 待机 | 没有会话在忙、没人需要你 | 默认环；整体呼吸缩放 1 → 1.025 | 呼吸 4 秒一次，缓入缓出，循环；6–12 秒随机眨眼一次（两瓣纵向压到 0.1 再回来，0.18 秒）；**2 分钟无事则停止呼吸，静止** | 品牌色 55% |
| 在忙 | 至少一个会话在忙 | 缺口绕环流动，缺口头部一颗白色光点 | 1.6 秒一圈，匀速，循环（与菜单栏图标 A7 同一节奏） | 品牌色 |
| 需要你 | 有会话 `attention` 非空 | 眼睛睁大（振幅 15.5、两瓣 1.1 倍）、缺口变短 | 进入时跳两下（弹簧 response 0.35 / damping 0.55，约 0.9 秒），天线左右招手 ±22°，招 3 秒后停；之后静止睁眼直到被处理，每 60 秒再轻招一次 | 品牌色，招手时亮 |
| 断了（担心） | 有会话 `interrupted` | 环断开一大截（缺口 30%）、眼角下垂、琥珀色；身体轻微压扁 0.95 | 进入时 0.4 秒形变，之后静止 | 琥珀色，天线垂下 |
| 找回 / 完成（开心） | `restored` 事件；用户点了「找回」成功 | 眯眼笑（只画两瓣的上半弧）、两颗 `sparkle` | 跳一下（0.7 秒），笑 2 秒后回到当时该有的动作 | 品牌色 |
| 睡觉（勿扰） | 勿扰中；用户离开超过 5 分钟 | 闭眼（只画下半弧、振幅 6）、整体 80% 不透明、`zzz` | `zzz` 3 秒一次缓慢冒出；其余静止 | 灰色；有攒着的事时角标显示数量 |

- 优先级（决定「此刻是什么动作」）：睡觉 > 需要你 > 断了 > 在忙 > 待机，与 A7 一致（勿扰最高，因为那是用户明说的）
- 「开心」「点头」是一次性演出，播完回到按优先级算出的动作
- 悬停：眼睛朝鼠标方向偏 1–2 pt（看你一眼），不改动作
- **减少动态效果**（系统设置）：所有循环停掉，动作之间只做 0.2 秒淡入淡出；跳、招手改为天线灯闪一次
- 尺寸：小 48 / 中 64（默认）/ 大 88 pt（身体宽度），全部矢量，按屏幕缩放清晰

## 5. 事件 → 演出

原则：**姿态看快照，演出看事件**。持续动作（待机 / 在忙 / 需要你 / 断了）由 `ls --json` 快照按 §4 的优先级算；一次性演出与气泡由 `events --json` 的新事件触发（A13）。这样漏掉一条事件也不会让宠物卡在错的动作上。时序图：`diagrams/pet/sequence-event-performance.html`（源 `diagrams/src/pet-sequence.sequence.json`）。

| 事件（T2.1） | 演出 | 气泡（默认「正常」档） | 语音（开启时） |
|--------------|------|------------------------|----------------|
| `started` | 天线灯闪一下 | 无 | 无 |
| `working` | 进入「在忙」（来自快照） | 无 | 无 |
| `finished` | 点一下头（0.3 秒） | 无（「话痨」档才有，且合并） | 无（「话痨」档才说） |
| `needs_you` | 跳两下 + 招手，然后停在「需要你」 | **有，常驻**：标题「api-review 需要你」，按钮「去看看」「稍后」；该会话 `attention` 清掉后自动收起，小莫点一下头 | **有**：只读标题 |
| `interrupted` | 进入「担心」 | **有**：「fix-invoice 断了」，按钮「找回」「先不」；20 秒后收成角标 | **有** |
| `restored` | 开心 | **有**，5 秒：「3 个会话都找回来了」 | 有 |
| `renamed`、`ended`、`closed` | 无（用户自己做的事，不必回声） | 无 | 无 |

合并与节流（由「导演」Director 负责，纯逻辑、可单测）：

- **合并窗口 1.5 秒**：窗口内同类事件合成一个气泡（「2 个会话在等你」「有 3 个会话断了」）；`needs_you` 不等窗口，立即演
- **一次只有一个气泡**：更高优先级（需要你 > 断了 > 找回 > 做完）顶替正在显示的；被顶掉的不再弹，计入面板与角标
- **同一会话同一类事件 60 秒内不重复开口**（`needs_you` 除外：它清掉又来，说明是新的一次确认）
- **语音每 30 秒最多一句**；新句子来时旧句子没说完就丢弃旧的，不排长队
- 开机时对账发现的一批 `interrupted`（A8）：只出一个气泡「有 N 个会话断了」，按钮「全部找回」

## 6. 什么时候说话

### 6.1 三档「话多不多」

| 档位 | 冒泡 | 语音（开启时） |
|------|------|----------------|
| 安静 | 只有「需要你」 | 只有「需要你」 |
| **正常（默认）** | 需要你、断了、找回 | 需要你、断了 |
| 话痨 | 以上 + 每轮做完（合并）、开工 | 以上 + 找回、做完 |

### 6.2 不打扰的场合

| 场合 | 怎么判断 | 宠物怎么做 |
|------|----------|------------|
| 勿扰（宠物右键：一小时 / 今天 / 直到我关掉） | 自己的设置 | 睡觉；不冒泡、不出声；「需要你」只让天线灯慢慢亮、角标 +1；勿扰结束时给一个汇总气泡「你不在的时候有 3 件事」 |
| 系统专注模式 | `INFocusStatusCenter`（T3.3 探针，可用才接） | 同勿扰；设置里「跟随系统专注模式」默认开 |
| 原生全屏 App | 不加 `.fullScreenAuxiliary`，系统自动不在全屏空间显示 | 退场；事情攒着，回到桌面再汇总 |
| 演示 / 非原生全屏（Keynote 放映、视频全屏） | 在屏窗口边界覆盖整块显示器（§2.1 的近似法，排除 Dock 与自己） | 退场，同上 |
| 共享或镜像屏幕 | 镜像可由 `CGDisplayIsInMirrorSet` 判断；共享屏幕没有公开 API（T3.2 探针） | 镜像时退场；共享无法判断时靠右键「演示模式」一键退场 |
| 你在开会（麦克风或摄像头被别的 App 占用） | `AVCaptureDevice` 的 `isInUseByAnotherApplication`（T3.4 探针） | 不出声，气泡照常 |
| 你不在电脑前（无键鼠输入超过 5 分钟） | `CGEventSource.secondsSinceLastEventType` | 不出声（没人听）；睡觉；回来后汇总 |

### 6.3 气泡

- 结构：标题行（状态图块 + 会话名与一句状态 + 正在朗读的小喇叭 + 关闭）、正文（不超过 2 行，中文 30 字 / 英文 80 字符内）、动作区（主动作着色，最多 3 个按钮）
- 位置：默认在宠物左上方，尾巴指向天线；靠近屏幕边缘时自动翻到另一侧
- 停留：需要你 = 常驻直到处理；断了 = 20 秒后收成角标；找回 = 5 秒；做完 = 6 秒；鼠标悬停时暂停计时；点正文 = 主动作；Esc 不可用（宠物不抢键盘），用关闭按钮
- 文案：会话名 + 平实的状态，偶尔一点温度，不用 emoji；**正文不出现任何会话内容**（提示词、命令全文都不读），`needs_you` 用我们自己的话按 `reason` 说（权限 / 提问 / 等输入），不转述 Claude 的原文（钩子探针 N1、N3：原文本来也只是一句通用的话）

| 场合 | 中文 | English |
|------|------|---------|
| 需要你（权限） | api-review 需要你 · Claude 想运行一条命令，在等你点头。 | api-review needs you · Claude wants to run a command and is waiting for your nod. |
| 需要你（提问） | api-review 有个问题 · 它在等你选一个答案。 | api-review has a question · It is waiting for you to pick an answer. |
| 断了 | fix-invoice 断了 · 别慌，进度都在。点一下就找回来。 | fix-invoice dropped · Nothing is lost. One click brings it back. |
| 开机发现断了 | 有 3 个会话断了 · 进度都在，要全部找回吗？ | 3 sessions dropped · Nothing is lost. Bring them all back? |
| 找回 | 3 个会话都找回来了 · 文明会延续，对话也是。 | All 3 sessions are back · Civilizations carry on. So do conversations. |
| 做完（合并，话痨档） | 2 个会话在等你 · term-kit 和 billing 这一轮都做完了。 | 2 sessions are waiting · term-kit and billing finished their turns. |
| 勿扰结束汇总 | 你不在的时候有 3 件事 · 1 个需要你，2 个做完了。 | 3 things happened while you were away · 1 needs you, 2 finished. |

图：`diagrams/pet/bubbles-zh-light.png`、`bubbles-en-dark.png`（另两套组合 `--pet-sheets` 也会生成，已目检，为控制仓库体积不入库）。

### 6.4 语音

- 本机 `AVSpeechSynthesizer`，**只读气泡标题**，声音跟界面语言（A10）走；语速取系统默认，音量跟随系统
- 默认**关**；首次打开宠物时问一次（「要不要让小莫开口说话？」）
- 设置：开关、声音（列本机已装的该语言声音，优先高级 / 增强）、「下载更好的声音」跳转系统设置、试听
- 可选「轻提示音」代替语音（用系统自带提示音，不引入音频素材），默认关

## 6.5 互动模式（用户 2026-10-16 修订，取代 §6.1 的三档「话多不多」）

原则：**先用表情和动作示意，用户来找它，它再开口。**「需要你」时不直接把消息框怼到用户面前。

| 模式 | 需要你时的表达 | 用户怎么进入互动 |
|------|----------------|------------------|
| 安静 | 只有表情与动作（睁大眼、招手、天线灯） | 点击小莫 → 消息框 |
| **日常（默认）** | 表情与动作 + 身边浮起**半透明的小气泡**（一两个字的口头禅：「嗯？」「…」「有件事～」），冒几秒后淡出，同一件事不反复冒 | 鼠标悬停在小莫或小气泡上 → 展开为完整消息框（标题、正文、去看看 / 稍后） |
| 积极 | 表情与动作 + 消息框直接弹出 | 直接点按钮 |

- **平时半透明**：没有要互动的事时，小莫整体约 55% 不透明；想找用户（需要你、断了、刚找回）或鼠标悬停时，0.3 秒过渡到 100%；事情处理完、鼠标离开约 2 秒后回到半透明
- 消息框在鼠标移开后延迟约 1.5 秒收起，鼠标回到框上则不收，避免移动途中闪烁
- 口头禅中英各一组、随机轮换；勿扰时三种模式都只保留表情（不冒泡、不弹框）
- 与通知中心的关系不变：A23 的「提醒路由」——小莫在桌面上可见时，由它负责；它不在时才发系统通知

## 7. 交互

| 操作 | 结果 |
|------|------|
| 单击小莫 | 打开状态栏面板（与点菜单栏图标同一个面板，A17） |
| 单击气泡 / 主动作 | 做主动作：去看看 = 在终端打开该会话（T1.2）；找回 = `cc-moss restore`；看看 = 打开面板并筛到该状态 |
| 拖动 | 任意摆放；松手时离屏幕边缘 24 pt 内自动贴边 |
| 记住位置 | 存「哪块屏幕 + 相对该屏可见区域的位置」；屏幕拔掉或分辨率变了，夹回可见区域；那块屏幕不在了就回主屏右下角 |
| 右键 | 打开面板 / 勿扰一小时 / 今天勿扰 / 静音 / 演示模式 / 收到边上 / 宠物设置… / 隐藏宠物 |
| 收到边上 | 身体一半藏进屏幕边，只露眼睛；有事时探出来 |
| 悬停 | 眼睛看向鼠标；0.8 秒后出提示「2 在忙 · 1 需要你」 |
| 连点三下 | 小彩蛋：开心一下（不出声） |
| VoiceOver | 一个辅助元素，标签读当前状态与气泡标题；右键菜单动作都可用 |

默认位置：主屏右下角、Dock 之上 24 pt。默认是否开启宠物见 §13。交互原型：`diagrams/pet/prototype-interaction.html`（浏览器打开即可：点事件看演出与合并，拖动、单击、右键，切勿扰 / 全屏 / 话多档位；`?demo=needs|finished2|dnd|menu` 直接进对应场景）。

## 8. 窗口技术

```text
NSPanel(styleMask: [.borderless, .nonactivatingPanel])
  isFloatingPanel = true, level = .floating        // 在普通窗口之上，不盖全屏与系统界面
  collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]   // 不加 .fullScreenAuxiliary
  backgroundColor = .clear, isOpaque = false, hasShadow = false        // 阴影自己画，贴着形状
  becomesKeyOnlyIfNeeded = true, hidesOnDeactivate = false
  App 本身已是 LSUIElement（A11），不出现在 Dock 与 ⌘Tab
```

- 窗口只比宠物 + 气泡大一点，随气泡出现临时变大；不画东西的地方不接收点击：内容视图的 `hitTest` 只认身体与气泡的形状，其余返回 nil；T3.2 用探针确认透明区域的点击确实落到下面的窗口，不行就按鼠标位置动态切换 `ignoresMouseEvents`
- 两块屏幕：位置按屏幕记；监听 `NSApplication.didChangeScreenParametersNotification` 重新夹位
- 菜单栏 App 与宠物共用一个 `AppModel` 与 Watcher（A4），宠物不另起刷新

## 9. 动画实现与资源预算

- 小莫由一组 `CAShapeLayer` 组成（身体、眼、天线、灯），路径由 Continuum 参数算出；每个动作是一组参数，切换动作 = 一次 `CABasicAnimation` / `CASpringAnimation`；循环（呼吸、缺口流动）是 `repeatCount = .infinity` 的图层动画，**由渲染服务器执行，本进程不逐帧计算**
- 帧率：循环动画 `preferredFrameRateRange` 上限 30 fps，一次性演出 60 fps
- 停帧：窗口被完全遮住（`occlusionState`）、屏幕休眠、退场、2 分钟无事时，移除循环动画（不是暂停计时器，而是没有动画在跑）
- 气泡用 SwiftUI（静态内容 + 出现消失的短过渡），不用 `TimelineView`

| 项 | 预算 | 怎么测（T3.2 / T3.4 验收） |
|----|------|------------------------------|
| 空闲 CPU（待机 / 在忙循环中） | 本进程 < 1%（60 秒平均） | 活动监视器 / `top -pid` 各动作 60 秒采样；同时看 WindowServer 增量 |
| 内存 | 宠物开启后整个 App 增量 < 20 MB，总量 < 60 MB | `footprint` |
| 能耗 | 活动监视器「能耗影响」空闲时为低 | 同上 |
| 帧率 | 循环 ≤ 30 fps | Instruments Core Animation |

隐私：宠物只用 App 已有的数据（`ls --json` 快照与 `events --json` 事件），不读会话内容、不读屏幕内容（全屏判断只用窗口边界）、不联网；语音在本机合成，只读气泡标题；003 不碰麦克风（「在开会」只读设备是否被占用）。

## 10. 给 004、005 留的位置（不在本阶段设计）

- **气泡动作区**：气泡由「标题 / 正文 / 详情区 / 动作区」四段组成，本阶段不用详情区。004 在详情区放完整的待确认内容（等宽字体、最多 6 行、可滚动），在动作区放「同意 / 拒绝」；超时或 App 不在时退回终端确认（§1.1）。原型图里的虚线框就是这个位置
- **语音入口**：天线就是麦克风。005 里「小莫说完 → 提示音 → 天线灯变成录音指示、旁边出现『在听…』胶囊 → 说完自动断句」；按住小莫（或全局快捷键）= 按住说话的备选。原型图右下的虚线胶囊就是这个位置
- **导演的发言队列**（§5 的「一次一个气泡、语音不排长队」）在 005 里扩展成「轮流说话」的管理者：小莫说话时不录音，录音时不说话
- 接口不变：004、005 仍然只消费同一条事件流，加的是新的事件种类与动作，不加新的数据通道

## 11. 模块与实现拆分

| 模块 | 职责 | 落在 | 测试 |
|------|------|------|------|
| `PetDirector` | 事件 + 快照 + 环境（勿扰、全屏、离开、档位）→ 此刻的动作、要演的一次性演出、要显示的气泡、要说的话；合并、节流、优先级、攒着 | MossKit（无界面，可单测） | Swift Testing 表驱动：合并窗口、顶替、60 秒去重、勿扰攒着与汇总、档位矩阵 |
| `PetGeometry` | Continuum 参数 → 路径（同点数），六个动作的参数表 | MossKit | 点数恒定、半弧截取、参数边界 |
| `PetWindow` | `NSPanel` 配置、拖动、贴边、记位置、多屏、点击穿透 | CCMoss | 位置夹取的纯函数单测 + 真机走查 |
| `PetLayer` | 图层树、动作切换动画、循环与停帧、减少动态效果 | CCMoss | `--pet-sheets` 出图 + 录屏 + CPU 采样 |
| `PetBubble` | 气泡视图与动作 | CCMoss | `--render` 出图（中英、深浅） |
| `PetEnvironment` | 全屏 / 镜像 / 离开 / 麦克风占用 / 专注模式的探测 | CCMoss（可注入） | 注入假实现驱动 Director |
| `Speaker` | `AVSpeechSynthesizer` 包装，声音选择与回退，30 秒限频 | CCMoss（可注入） | 注入假实现断言「说了什么、没说什么」 |

- T3.2：`PetGeometry`、`PetWindow`、`PetLayer`、设置里的开关与大小（`SettingsCard`，与 T2.4 同一套）
- T3.3：`PetDirector`、`PetBubble`、`PetEnvironment`；与 T2.3 通知中心共用「这个事件要不要打扰」的口径（同一事件，宠物开着时通知中心是否还发，见 §13）
- T3.4：`Speaker` 与语音设置

## 12. 风险与开放问题

| # | 项 | 处理 |
|---|----|------|
| P1 | 形象版权 | 全部自绘：Continuum 曲线、圆角矩形、圆、SF Symbols `sparkle` `zzz`；评审时逐项核对来源；仓库里不放任何外来图片 |
| P2 | 透明区域的点击穿透是否可靠 | T3.2 探针；不行则按鼠标位置切换 `ignoresMouseEvents` |
| P3 | 屏幕共享无法判断 | 右键「演示模式」兜底；探针看有没有可靠的近似 |
| P4 | `INFocusStatusCenter` 在 ad-hoc 签名的 macOS App 上是否可用 | T3.3 探针；不可用就只有宠物自己的勿扰，并在设置里说明 |
| P5 | 宠物与通知中心重复提醒 | 默认：宠物开着且未退场时，「需要你」只由宠物提醒，通知中心不再弹；宠物退场（全屏、勿扰）时照常走通知中心。待拍板 |
| P6 | 打扰过度 | 默认「正常」档 + 语音关；所有阈值（1.5 秒、60 秒、30 秒、5 分钟）集中在 Director 的常量里，走查后可调 |
| P7 | 「需要你」的 6–9 秒延迟（钩子探针 N2） | 宠物如实反映：那条事件到了才招手；不猜 |
| P8 | Core Animation 循环在 WindowServer 里的开销 | 预算同时看 WindowServer；超标就把呼吸改成每 30 秒一次的单次动作 |

## 13. 需要你拍板

1. **形象方向**：A 环 / **B 小莫（推荐）** / C 轨道
2. **名字**：中文「小莫」、英文「Mo」可以吗？
3. **宠物默认开不开**：建议首次启动引导（A22）里问一次，默认勾选「开」——它是这一版的亮点
4. **默认「话多不多」**：安静 / **正常（推荐）** / 话痨
5. **语音默认**：**关（推荐）**，首次打开宠物时问一次；还是默认开？
6. **勿扰时「需要你」破不破例**：**不破例（推荐，只亮天线灯 + 角标）**；或者破例冒泡但不出声
7. **跟随系统专注模式**：**默认跟随（推荐）**
8. **单击小莫做什么**：**打开状态栏面板（推荐）**；或者直接在终端打开最需要你的那个会话
9. **宠物开着时通知中心还发不发「需要你」**（P5）：**宠物在场就不发（推荐）**，退场时照发
10. **默认大小与位置**：中号 64 pt、主屏右下角，可以吗？
11. **彩蛋**（连点三下开心一下、悬停时眼睛看鼠标）：留着还是去掉？

## 14. 出图索引

| 文件 | 内容 |
|------|------|
| `diagrams/pet/directions-zh-light.png`、`directions-en-dark.png` | 三个方向 × 五个动作 |
| `diagrams/pet/poses-zh-light.png`、`poses-en-dark.png` | 小莫六个动作与参数说明 |
| `diagrams/pet/frames-zh-light.png`、`frames-en-dark.png` | 在忙、眨眼、需要你的关键帧 |
| `diagrams/pet/bubbles-zh-light.png`、`bubbles-en-dark.png` | 气泡示例（含 004、005 的预留位） |
| `diagrams/pet/prototype-interaction.html` | 可交互原型（导演逻辑的可玩版） |
| `diagrams/pet/sequence-event-performance.html` | 事件 → 演出时序图（archify，源 `diagrams/src/pet-sequence.sequence.json`） |

重新生成 PNG：`apps/macos/scripts/swift.sh build && apps/macos/.build/debug/CCMoss --pet-sheets <目录>`；入库前转成不带透明通道的 PNG 以控制体积。
