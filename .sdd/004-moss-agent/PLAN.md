# PLAN：004 Stage 3 Moss agent（4 Phase × 11 Task + R1）

## 配套文档
- 权威设计：`DESIGN-moss-agent.md`（D1–D4 已拍板，A1–A18，C1–C9）
- 立项依据：`notes/drivers-evidence.md`；现状：`notes/current-state-baseline.md`；锚点：`notes/anchors.md`（2026-10-18）
- 外部事实：`notes/claude-code-facts.md`、`notes/slack-presence-facts.md`；参考：`notes/reference-study.md`
- 图：`diagrams/architecture-moss-agent.html`、`diagrams/sequence-ask.html`

## 铁律
- 本 PLAN 只编排不实施；每个任务先有 spec（`tasks/draft/` → gate → `todo/`）再动代码
- 实施落 `feature/004-moss-agent` 的子分支，走 PR squash 合入 `main`（`.sdd/config.md` `{REPOS}`）
- 合入前跑 CI 同款：`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --all`，
  App 侧 `apps/macos/scripts/swift.sh test`，官网改动 `pnpm -s build && pnpm -s test`
- 端到端只用独立 tmux socket + 临时 HOME + `CC_MOSS_STATE_DIR`，不碰真实会话与 `~/.claude`
- 公开仓：不写参考项目名、主机名、账号、token
- 文案中英双语、遵守 `002-menubar/notes/design-voice.md`；README 与 guide 中英同改

## 总览

| Phase | 主题 | Task | 版本 | 价值 |
|-------|------|------|------|------|
| P1 | 放手干 + 替你接住问题（MVP） | T1.1–T1.4 | 0.3.0（与 P2 合发） | 新会话不再卡权限；选择题不用进会话就能答 |
| P2 | Moss 住进来 | T2.1–T2.6 | 0.3.0；T2.5、T2.6 → 0.3.1 | 常驻守护、能和 Moss 对话、一键体检、会话归档、换供应商原地重开 |
| P3 | Moss 派活 | T3.1 | 0.4.0 | Moss 自己挑目录起 bypass 会话、做完回报 |
| P4 | Slack 与路由 | T4.1–T4.2 | 0.5.0 | 团队在频道里用 Moss；人不在时问题转到 Slack |
| R | 收口 | R1 | — | 整迭代重型评审 |

## 依赖图

```
P1  T1.1 探针 ──► T1.3 代答中枢 ──► T1.4 桌面答题
    T1.2 bypass 默认（独立，与 T1.1 并行）

P2  T2.1 守护进程 ──► T2.2 Moss 常驻 ──► T2.3 和 Moss 对话 ──► T2.4 Doctor（另需 T1.3）
P3                                        T2.3 + T1.3 ──► T3.1 派发
P4                                        T2.3 ──► T4.1 Slack ──► T4.2 在场与路由（另需 T1.4）
R1 ◄── 全部
```

- **关键路径**：T2.1 → T2.2 → T2.3 → T4.1 → T4.2
- **并行窗口**：第一波 T1.1 ∥ T1.2 ∥ T2.1（三者无依赖）；T1.3 等 T1.1 的探针结论；P2 链可与 P1 后半并行
- 每个 Phase 收尾发一个小版本（总览「版本」列）；发版沿用 release-protocol 与 0.2.x 的流程

## 任务

### T1.1 代答探针
- 目标：用真 Claude Code 定下三件事，结论写 `notes/ask-probe.md`：
  ①AskUserQuestion 的答案怎么回填（`updatedInput` 带答案 / `deny` + 理由当答案，两条都试）；
  ②bypass 下 PermissionRequest 是否触发；③代答 hook 超时或不表态后，会话是否照常显示自己的提示
- 文件：`notes/ask-probe.md`、探针脚本放 scratch（不入库），必要时 `notes/` 附录记录原始输入输出
- 内容：临时 HOME + 独立 tmux socket，起真 `claude` 会话，挂最小 PreToolUse / PermissionRequest hook，引导它调用 AskUserQuestion
- 验收：三问各有「行 / 不行 + 原始证据」；给 T1.3 选定回填方式
- commit：`docs(sdd): ask probe for answering AskUserQuestion`
- 依赖：无

### T1.2 新会话默认 bypass
- 目标：命令行与 App 起的新会话默认 bypass；`--safe` 单次普通模式；不卡在首次警告框
- 文件：`crates/moss-core/src/config.rs`（`[claude] permission_mode`）、`crates/cc-moss/src/cmd/new.rs`、
  `crates/moss-core/src/launch_args.rs`、`crates/moss-core/src/settings_json.rs` + `cmd/install`（受管写入 `skipDangerousModePermissionPrompt`）、
  `cmd/doctor`（检测管理策略禁用 bypass）、`apps/macos/Sources/MossKit/NewSession.swift`、`apps/macos/Sources/CCMoss/SessionForms.swift`、
  通用设置页、`docs/guide*.md`、`CHANGELOG.md`
- TDD：配置默认值与解析；`new` 的 claude 命令行（默认 / `--safe` / `default_args` 叠加不重复）；受管写入可撤销；策略禁用时退回
- 验收：临时 HOME 下 `new` 与 App 表单起的会话都带 bypass；tmux 里不出现警告框；`--safe` 不带
- commit：`feat: new sessions start in bypass mode`
- 依赖：无

### T1.3 代答中枢（CLI）
- 目标：会话问选择题（或 `--safe` 会话问权限）时，问题被完整记下，任何一方答了就交回会话；不开或没人接手时行为和现在一样
- 文件：`crates/cc-moss/src/cmd/hook.rs`（新子命令 `hook ask`，独立契约）、`crates/moss-core/src/ask.rs`（新：ask 文件协议）、
  `crates/cc-moss/src/cmd/ask.rs`（新：`ask list|answer|watch --json`）、`settings_json.rs`（登记 PreToolUse(AskUserQuestion) 与 PermissionRequest）、
  `crates/cc-moss/tests/fixtures/fake-claude`（能发 PreToolUse 并读回 hook 输出）、e2e 测试、guide
- TDD：ask 文件读写与先答者胜（`O_EXCL`）；超时 / 出错不表态；PostToolUse 到达即撤回；按 T1.1 定的方式回填；开关关闭时立即返回
- 验收：假 claude e2e：问 → `ask answer` → 会话收到答案；超时走原生提示；代答 hook 不影响现有 6 个 hook 的耗时
- commit：`feat: catch a session's question and answer it from outside`
- 依赖：T1.1

### T1.4 桌面答题
- 目标：面板和小莫上直接答会话的问题；「它在等你」时能直接回一句
- 文件：`apps/macos/Sources/MossKit/`（ask 解码与监听）、`apps/macos/Sources/CCMoss/`（面板答题卡、`PetBubble` detail 区、通知动作、设置「替会话接住问题」与等待时长）、
  `AlertRouter.swift`（ask 也只去一处）、`Render.swift`（离屏渲染四种卡片）、guide、官网截图
- TDD：ask 解码；卡片状态（单选 / 多选 / 自由输入 / 允许拒绝）；路由；答完撤回
- 验收：离屏渲染四种卡片；`--click` 探针点选后 `ask answer` 被调用；真机：在面板答完，会话继续
- commit：`feat(app): answer a session's question from the panel or Mo`
- 依赖：T1.3

### T2.1 守护进程
- 目标：`cc-moss daemon` 由 launchd 托管常驻，挂了自动回来；App 退出不影响它
- 文件：`crates/cc-moss/src/cmd/daemon.rs`（新：`run|start|stop|status`）、`crates/moss-core/src/daemon/`（单实例锁、状态文件、日志轮转、重启阶梯）、
  LaunchAgent plist 生成与 `install` / `uninstall`、`doctor`、App 设置新页「Moss」（守护进程状态、开关）
- TDD：plist 生成；单实例锁；重启阶梯计时；状态文件
- 验收：`kill -9` 后 launchd 拉回；`daemon status` 准确；卸载干净
- commit：`feat: a resident cc-moss daemon under launchd`
- 依赖：无

### T2.2 Moss 常驻
- 目标：Moss 是一个一直在的会话，列表里固定显示、结束不掉，被杀后自动恢复并读台账接着干
- 文件：`crates/moss-core/src/moss/`（新：档案模板布置与受管升级、会话拉起、判活）、模板目录（CLAUDE.md、SECURITY、IDENTITY、SOUL、USER、journal、memory、`.claude/settings.json` 防护 hook、skills 骨架）、
  `cmd/daemon`（守护 Moss）、`pick` / `close` / 面板（保护 Moss）、App 面板固定行
- TDD：模板布置不覆盖用户改过的文件；防护 hook 拒绝改自身 settings / 写目录外；判活宁漏不误杀；阶梯重启
- 验收：独立环境下守护进程拉起 Moss；`tmux kill-session` 后按阶梯回来且读了台账；`pick` 与面板里结束不了
- commit：`feat: Moss, a resident session cc-moss keeps alive`
- 依赖：T2.1

### T2.3 和 Moss 对话
- 目标：在面板里和 Moss 说话、看它回复；Moss 知道各会话的动态（需要你 / 断了 / 做完了）
- 文件：`crates/moss-core/src/moss/inbox.rs`（注入 + 回读确认）与 `reply.rs`（Stop 标记 + 对话记录认领，带降级）、事件摘要批处理、
  `moss-sessions` skill、App 面板「Moss」对话视图、回复控制标记解析（`[[silent]]` `[[buttons:]]`）
- TDD：注入确认与重试；认领回复（含格式变化时降级）；摘要合并与限频；控制标记
- 验收：独立环境下发一句话 → 面板显示 Moss 回复；会话断了 30 秒内 Moss 知道
- commit：`feat: talk to Moss from the panel`
- 依赖：T2.2

### T2.4 Doctor
- 目标：一键让 Moss 体检会话：能直接整理的整理，要问的先问
- 文件：`cmd/doctor`（新 `--sessions --json`）、`moss-doctor` skill（预制 prompt）、需要的整理命令（批量关闭 / 补名字 / 便条）、
  面板「让 Moss 体检」入口、每天一次的定时（可关）
- TDD：会话事实各类判定；破坏性动作必经 ask
- 验收：演示数据下体检报告与预期一致；关闭类动作先弹问题
- commit：`feat: Moss checks up on your sessions`
- 依赖：T2.3、T1.3

### T3.1 派发
- 目标：Moss 能自己起一个 bypass 会话去干活：挑目录、拿不准问你、做完回报来源
- 文件：`crates/cc-moss/src/cmd/dispatch.rs`（新：`dispatch` 与 `dispatch resolve`）、`audit.jsonl`、暂停开关（设置与命令）、
  `moss-dispatch` skill（L0–L3 分级）、Moss 防护 hook 的「产出文件必派发」规则、完成回报
- TDD：目录候选打分；低置信度走 ask；审计行；暂停时拒绝；完成事件回到来源
- 验收：独立环境下 Moss 收到任务 → 派发 → 子会话 Stop → Moss 回报
- commit：`feat: Moss hands work to new sessions`
- 依赖：T2.3、T1.3

### T4.1 Slack
- 目标：Moss 绑定到一个 Slack App，在允许的频道里被 @ 时回复（只回频道，不接私聊），带自定义人设
- 文件：`crates/moss-core/src/slack/`（Socket Mode：连接、ack、重连、去重；Web API：发消息、线程）、钥匙串存 token、
  App 设置「Slack」页（预填 manifest 链接、粘贴 token、允许的频道、提醒频道、人设名字与头像）、guide
- TDD：信封 ack 与去重（录制样本）；频道过滤（私聊、非允许频道、bot 自己的消息都忽略）；人设字段
- 验收：开发用工作区里 @Moss 得到线程回复；私聊无响应
- commit：`feat: Moss on Slack`
- 依赖：T2.3

### T4.2 在场与路由
- 目标：问题和提醒送到你在的地方：在场去桌面，离开或锁屏去 Slack；绑定 Slack 后桌面安静；小莫可一键回到桌面
- 文件：`crates/moss-core/src/presence.rs`（空闲秒数、锁屏、睡眠、状态机）、路由（守护进程内，`AlertRouter` 迁移或对齐）、
  Slack 端 ask 渲染（Block Kit）与回答回写、撤回、App 设置「交给谁」、小莫菜单「回到桌面」、菜单栏待答数
- TDD：在场状态机（含回到在场的防抖）；路由表；状态变化时重新路由未答的 ask；两边先答者胜与撤回
- 验收：模拟空闲 / 锁屏下 ask 去向正确；Slack 点按钮后会话继续、桌面卡片撤回
- commit：`feat: questions follow you between the desktop and Slack`
- 依赖：T4.1、T1.4

### R1 收口评审
- 目标：整迭代重型评审（安全维度必有：bypass、代答 hook、Moss 权限、Slack token 与频道）+ 交付报告四桶
- 依赖：T1.1–T4.2

## Kanban

| Task | 仓 | 依赖 | 状态 | SDD | 验收锚点 |
|------|----|------|------|-----|----------|
| **T1.1** 代答探针 | cc-moss | — | ✅ Done | done/T1.1-ask-probe.spec.md | 三问各有证据，选定回填方式 |
| **T1.2** 新会话默认 bypass | cc-moss | — | ✅ Done | done/T1.2-bypass-default.spec.md | 新会话带 bypass、不卡警告框 |
| **T1.3** 代答中枢（CLI） | cc-moss | T1.1 | ✅ Done | done/T1.3-ask-broker.spec.md | 假 claude 问答闭环、超时走原生 |
| **T1.4** 桌面答题 | cc-moss | T1.3 | ✅ Done | done/T1.4-desktop-answer.spec.md | 面板答完会话继续（侧栏 + 待处理页 + 小莫气泡，真机验收通过） |
| **T2.1** 守护进程 | cc-moss | — | ✅ Done | done/T2.1-daemon.spec.md | kill -9 后自动回来 |
| **T2.2** Moss 常驻 | cc-moss | T2.1 | ✅ Done | done/T2.2-moss-resident.spec.md | 杀掉后阶梯恢复、列表里结束不了 |
| **T2.3** 和 Moss 对话 | cc-moss | T2.2 | ✅ Done | done/T2.3-talk-to-moss.spec.md | 面板发一句、看到回复（AMEND-1～4，真机验收通过） |
| **T2.4** 体检 | cc-moss | T2.3,T1.3 | ✅ Done | done/T2.4-moss-fixes.spec.md | 能修的直接修、要定的弹选项、体检简报 |
| **T2.5** 会话归档与清理 | cc-moss | T2.4 | ✅ Done | done/T2.5-archive.spec.md | 30 天不用自动归档；清理经 Moss 确认、对话记录进废纸篓 |
| **T2.6** 换供应商后原地重开 | cc-moss | T2.4 | ✅ Done | done/T2.6-provider-switch.spec.md | 能热生效的只提示；要重开的一键原地重开，pane 不变、忙的等这轮结束 |
| **T3.1** 派发 | cc-moss | T2.3,T1.3 | ⬜ Backlog | — | 派发 → 完成 → 回报 |
| **T4.1** Slack | cc-moss | T2.3 | ⬜ Backlog | — | 频道 @Moss 有线程回复、私聊不回 |
| **T4.2** 在场与路由 | cc-moss | T4.1,T1.4 | ⬜ Backlog | — | 离开时问题去 Slack、答完桌面撤回 |
| **R1** 收口评审 | cc-moss | T1.1–T4.2 | ⬜ Backlog | — | 评审报告与交付四桶 |

## 日志
- G1：设计批准 approved-by=user date=2026-10-18
- G2：编排确认 approved-by=user date=2026-10-18（「确认，开始第一波」）
- G3：T1.1、T1.2、T2.1 spec 过 `sdd-gate.sh`（2026-10-18）
- T1.1 ✅ 2026-10-18：探针结论 `notes/ask-probe.md`（f150399），controller 抽查 verify 全绿
- DESIGN-AMEND: A5 等待 90s→300s、A6 回填定为 PermissionRequest + updatedInput.answers、K1 改写 reason=T1.1 实测 approved-by=controller（不弱化任何验收，按探针事实修订）date=2026-10-18
- AMEND: T1.2 `--safe` 同时去掉 default_args 里的权限参数（加强，不弱化）reason=「--safe = 这一次普通模式」 approved-by=controller date=2026-10-18
- T1.2 ✅ 2026-10-18：CI 同款 522 通过、Swift 297 通过；controller 复跑 verify 9 项全绿
- T2.1 合入档案分支 1020d6c（合并树 CI：Rust 553、Swift 302 全绿）；真机：独立 probe label + 临时状态目录，`kill -9` 后约 22 秒由 launchd 拉回、单实例第二份退出 1、已清理；发现 K7（TCC），程序在 `~/Documents` 下会被卡住
- 2026-10-18 教训：T1.3 真机补测的两个 claude 会话加载了用户全局的 cc-moss hook，登记进用户真实 registry，探针结束后显示为「断了」，用户发现。已 `cc-moss close <session_id>` 清理。规避：探针启动真 claude 一律带临时 `CC_MOSS_STATE_DIR`（已写入派发纪律）。
- 2026-10-18 教训补充：临时 `CC_MOSS_STATE_DIR` 只挡 hook；用户 App / `ls` 刷新时经 `claude agents --json` 收编所有正在跑的 claude，探针会话仍进真实 registry（T1.3 修复的 I5 探针）。更早 T1.1 的两个探针会话被 kill 后显示「断了」、被「找回」进用户默认 tmux 一直运行。经用户同意结束并 close。规避：探针 claude 一律 `/exit` 正常退出、`ps` 确认无残留、报告列出全部 session_id，controller 事后核对清理
- T2.1 ✅ 2026-10-18：评审 I1–I4、M1–M12 已修（10 个回退必红变异全红）；合并树 CI Rust 599、Swift 303 全绿；M13 转 T2.2
- T1.2 → main：PR #36（45d7ce4），CI 全绿，用户授权「T1.2 好了就合并」
- T1.3 ✅ 2026-10-18：评审 I1–I4 + Minor 已修（变异全红），I5 探针：Claude Code 采用第一个给出决定的 PermissionRequest hook，不拖住用户的 hook；合并树 Rust 630 全绿
- AMEND: T1.3 契约（asks/done、临时文件 + hard_link 落地、`invalid-answer`、`tool.input_hash`）reason=评审修复 approved-by=controller（派发时预先批准）date=2026-10-18
- T2.2 合入档案分支 c7aaaab（合并树 Rust 666、Swift 309 全绿），独立评审中。真机：全新目录以 bypass 启动 Claude Code 2.1.302 不出现「信任此文件夹」与 bypass 警告框，直接可输入（Moss 首次启动不会卡）；探针按规则 `/exit`，被 reconcile 收编的一条已 close
- 裁决（T2.2）：`daemon stop` 由 CLI 一侧顺带停掉 Moss（升级的 kickstart 不停）；网站 FAQ「联网」一条补上 Moss 用你的 Claude Code 登录（与 guide 一致），T2.2 修正轮做
- T1.4 合入档案分支（合并树 Rust 670、Swift 362 全绿；高负载下两次全量各有带时限用例超时，单独与负载降后全量均绿）；裁决：`ask answer --json`（契约加项）；顺带修 T1.3 临时文件名在线程间撞名导致 `first_answer_wins` 偶发失败。待真机验收（真 Claude Code 在面板答一道题）
- T2.2 ✅ 2026-10-18：评审 I1–I10、M1–M10 已修（变异全红），Moss 的 Bash 进 Claude Code 沙箱并经真机探针确认；合并树 Rust 692、Swift 364 全绿；探针 4 个 claude 会话已在用户 registry 里 close，遗留测试 tmux 已关
- 裁决（T2.2 → T3.1）：沙箱里的 Moss 连不上 tmux；派发、发消息等需要 tmux 的动作**由守护进程代办**（Moss 写请求，守护进程校验、执行、审计），不把命令放出沙箱 approved-by=controller date=2026-10-18

- AMEND（T2.2 → T2.3 实施中，用户发起 2026-10-19）：「置顶怎么有两部分？… Moss agent 默认应该藏起来吧？因为 moss agent 有自己专门的 sidebar 菜单和单独的面板了」→ 会话列表不再固定显示 Moss 行（T2.2「App 面板固定行」作废），Moss 的提醒走侧栏点、待处理与小莫；`pick` / `close` 的保护不变 approved-by=user date=2026-10-19

### 2026-10-19
- P1 先发：从 main 开 `release/p1`（T1.3 + 修正 + T1.4，去掉 Moss 相关）装到本机真机验收；用户反馈后加：面板侧栏（会话 / 待处理 / Moss）、「待处理」队列页、小莫答题气泡重做、面板关闭时把题交给小莫；PR #34 的点击 / 行按钮修复并入。随后 `release/p1` 并回本分支
- T2.3 开工（G3 过 gate；用户批准三条默认口径）。Rust（inbox / 认领 / 动态摘要 / 控制标记）与 App（Moss 页）两路并行，独立评审 0 Critical / 5 Important / 8 Minor 全修；真机：Moss 加载用户 MCP 启动慢 → `--strict-mcp-config`；输入框没画出就粘贴 → 就绪判断
- T2.3 AMEND-1 内置动作优先、打字收进键盘图标；AMEND-2 Claude Code 命令（/usage /status 等）截屏回传，`/usage` `/status` 图形卡片，按账号类型（订阅 / API / 云）推荐；AMEND-3 统一快捷栏（按用量排序 + Moss `[[quick:]]` 推荐）；AMEND-4 会话搜索增强 + 「让 Moss 帮你找」+ `[[sessions:]]` 会话卡片
- T2.3 真机修正：Markdown 块级渲染、气泡 10 分钟内合并、对话分页 30/100、长回复折叠、动态灰字不显示、只回「做完了」的动态强制静默、残留输入自动清、上下文 500k（`CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000`）、加载状态、自动滚到底
- T2.4 体检开工：`doctor --sessions --json` 事实、守护进程代办通道 `moss act`（要点头的动作核对你的回复）、每天体检、体检简报、整理会话补名字 / 改正名字 / `sync-names`
- 性能：App 刷新约每秒一次（读 registry 产生的 -wal/-shm 与名字扫描写入自我唤醒）→ 写入时碰 `registry.changed`、App 只盯它，面板开 1.5 秒 / 关 5 秒最小间隔，`claude agents` 每 30 秒一次；实测 30 秒 30 次 → 40 秒 3 次
- tmux 名保留原名（中文、大小写、emoji），只换 tmux / shell 不认的字符（`notes/session-names.md`）
- 会话页体验：发送中锁定、发送后跟随、置顶分组（一张扁平列表修复悬停卡住）、点击行展开回复并预填 Claude Code 的下一句建议（`cc-moss suggest`，缓存按轮次 + 10 分钟）、齿轮统一 App 菜单、面板高度三档、Moss 退出会话列表、「移进 tmux / 整理并移进 tmux」（`cc-moss adopt`，进行中）
- 教训：替换开发版 CLI 时用 `cp` 原地覆盖正在运行的二进制，macOS 签名缓存失效、所有钩子被 SIGKILL 约 2 分钟。规避：一律 `cp` 到临时名再 `mv`
- 教训：真机探针起的 claude 会被 `claude agents` 收编进真实 registry、Moss 随之播报「断了」。规避：探针结束立刻 close 并核对
- 2026-10-19 收尾评审两轮：R1（Rust / App / 安全三路）与 R2（代码 + 安全复核）全部已修；R2 安全复核新发现 4 项 TOCTOU（文件工具竞态、actions 目录替换、profile --from、同意绑定不可见提议）与 `~/.claude.json` 读取，均已修；复核再报两处遗留（改名冒充确认按钮、`~/.claude.json*` 备份）也已修
- T1.4 ✅ T2.3 ✅ T2.4 ✅ 2026-10-19：真机验收（冷启动链接、历史卡片回执、会话卡片状态、提及徽标）通过，spec 归位 done/
- 版本裁决 2026-10-19：P1 未单独发版，与 P2 合发 0.3.0；T2.5 归档（用户新需求）发 0.3.1；P3→0.4.0、P4→0.5.0
- 2026-10-19 收尾期真机反馈并入 0.3.0：冷启动链接面板（激活 + 屏外定位）、历史会话卡片缩成回执、多题问题每题可自写（`--text N:TEXT`）、「推荐」标签、加载圈漂移
- STAGE-CLEAN: 2026-10-19 commits=[cc-moss:分支内保留，PR #35 squash 合入 main] review=[fixed:R1 全部 + R2 代码 10 + R2 安全 5+2 held:0] artifacts=0 kanban=ok
- 反馈车道 F1–F3（Moss 设置：Finder 打开家目录、去技术化文案、引导去快捷操作）延后到下一轮
- 2026-10-20：其他设备升 0.3.0 时发现 `remember` 插件的 `claude -p` 摘要被 `claude agents` 报成 interactive、经对账收进 registry（一台 118 条、一台 19 条）；修复 `fix/headless-adopt`（对账读进程参数、跳过 `-p`/`--print`，自动名不分大小写）并入 0.3.1
- 2026-10-20：用户新需求「cc-switch 换供应商后不用退出会话」→ 实测（`notes/provider-switch-probe.md`）：第三方之间热生效，删键 / 换固定模型要重开；立 T2.6，并入 0.3.1；T2.5、T2.6 过 G3
- AMEND-1 T2.5 2026-10-20（加宽，不弱化）：App 会话行为「没在运行」的会话加「归档」行内操作（spec 只写了 CLI `archive`）；`ls --json` 列出全部会话并带 `archived_at`，由 App 过滤。接口契约统一写在派发纪律里，两路 agent 并行实施
- 2026-10-20：T2.5、T2.6 实施完（Rust 305d615 / 91d16eb，App b6c4581 / 5cdd5d5，docs b279669）。verify 全有同名测试，spec 列出的变异全部变红；macOS Rust 864 + Swift 744 全绿；本地 Linux（rust:1.89 + tmux 3.5）除 moss_chat_e2e 4 项外全绿，这 4 项在 main 上同样失败，属本地镜像环境问题，另查。开发版已装本机，待真机验收（cc-switch 第三方↔第三方、切回官方各一次；归档与清理）
- 行为变化（rust-031，controller 认可）：restore / restart 占位后 60 秒内进程退出只改状态、不记 `interrupted` 事件，避免重开时误报「断了」
- 2026-10-20 真机反馈并入 0.3.1：F22 Moss 确认按钮折行不截断；F23 会话等你处理改为 Moss 页一张实时「等你处理」卡、Moss 不再播报（用户选定方案）；F24 Moss 跟随 API 供应商（0.3.0 回归：`--setting-sources project` 丢了用户 env）——供应商键写进 Moss 家 `.claude/settings.local.json`（600），沙箱 denyRead 该文件，真机 claude 实测更具体的 denyRead 压过 allowRead ["."]（cat 被拒）；另含结束并归档、拿回来的已结束会话回到列表、后台任务通知不当最后一句、齿轮菜单与关于页重排、回到底部朦胧条
- AMEND-2 T2.6 2026-10-20（精确化，不弱化；来自 rev031-rust M6/M9/M10）：(a) 重开判据 = 删键，或 env `ANTHROPIC_MODEL` 变了；顶层 `model`（`/model`）不再要求重开，重开时只在 env 指定了 `ANTHROPIC_MODEL` 才去掉 `--model`；(b) 每个会话对照它启动时读的那份 settings（钩子记录路径），守护进程同时盯这些文件，与 `[claude] config_dir` / `CLAUDE_CONFIG_DIR` / `~/.claude` 不一致时 doctor 提醒；(c) 重开队列记尝试次数，3 次失败放弃并记 `restart-failed` 事件，`--stale` 或抢占失败入队的条目在切换已生效后丢弃。R-RUST 合入 ea6672d…877d015；H1、L11 偏差 controller 认可（L11 改为写事务内重读 slot 比对，不加 `status <> 'running'`，以免崩溃会话无法自动归档/清理）
- 2026-10-20：R-SEC 合入（04c5750…c388837：SM1 Moss 的 key 走 `cc-moss moss provider-key`、SM2 Moss 归档要同意 + `archived_by`、SL1–SL6、信任提示、headless 精确读 argv）；ls_cli 偶发红定位为 macOS 首次执行新写脚本的检查超过 `claude agents` 3 秒，测试先预热桩（4a246a6，修前 3/16 红、修后 0/12）。合并树：macOS Rust 902 + Swift 803 全绿；Linux 本地 898/902，moss_chat_e2e 4 项为本地镜像环境问题（GitHub CI ubuntu 上通过）。开发版已装本机，doctor / Moss / 列表抽查正常
- STAGE-CLEAN: 2026-10-20 commits=[cc-moss:分支内保留，发版 PR squash 合入 main] review=[fixed:R-RUST 4 高 6 中 5 低 + R-APP 2 中 6 低 + R-SEC 2 中 7 低 held:0] artifacts=3 个已合入的 agent worktree kanban=ok
- 2026-10-20：PR #37 squash 合入 main（b394184，树与 feature/0.3.1 逐字节一致），打 v0.3.1，Release 与 Site 部署成功（4 平台包 + Homebrew；首页与 /changelog/ 上线）。发版前补修 F35（fseventsd 卡死拖死 App）、F36（同名会话：App 一律按会话 id 下命令）、CI 偶发（ls_cli 桩预热、argv 测试 dash exec）。本机、koo-mm4、koo-macstudio、koo-mm41 升 0.3.1（CLI + App + 钩子），doctor 全绿；升级前遗留的无头会话已归档（macstudio 6、mm4 1），升级后无新增。本机 App 的开发期 `cliPath` 覆盖已清除

## Round 0.3.1（里程碑，卡不归档）

- E2E: 2026-10-20 macOS Rust 902 + Swift 805、Linux 本地 898/902（moss_chat_e2e 4 项本地镜像环境，CI ubuntu 绿）、CI 全绿；真机：四台 doctor 全绿，本机 fseventsd 卡死下 App 仍刷新（轮询兜底）、同名会话按 id 归档
- DISTILL: (1) 系统服务调用（FSEvents、SMAppService）一律离开主线程并有兜底，主线程等系统守护进程 = 整个 App 卡死；(2) App 对单个会话下命令一律用会话 id，名字可重复；(3) 测试里新写的可执行桩先运行一次再计时（macOS 首次执行检查）；shell 桩用 `cmd; :` 防 dash exec 替换；(4) 远端 zsh 不做变量分词，批量参数走 xargs
- REPORT: 完成 = T2.5 归档与清理、T2.6 换供应商原地重开、真机反馈 F19–F36（F1–F3 除外）、三路评审全部修复、站点首页改版 + 更新记录时间线；新独立 = 「App 使用的命令行比 App 旧时提示并可一键改用最新」（用户 2026-10-20 反馈引出，待排期）；子任务 = 无；延后 = F1–F3（Moss 设置文案与引导）、P3 T3.1 派发、P4 T4.1 Slack / T4.2 在场与路由

## 反馈车道（真机反馈）

| # | 反馈 | 状态 |
|---|------|------|
| F1 | 设置 › Moss「在 Finder 中打开」Moss 的家 | ⬜ 待排期 |
| F2 | 设置 › Moss 文案太技术，按 C 端重做 | ⬜ 待排期 |
| F3 | 引导用户找到 Moss 对话页并点快捷动作 | ⬜ 待排期 |
| F4 | 体检的 tmux 名事实由守护进程代算 | ✅ 已修 |
| F5 | 小莫答题气泡：答案引用样式、切题不乱跳 | ✅ 已修 |
| F6 | 面板改名不生效 | ✅ 已修 |
| F7 | 行按钮跳动、点击选中、发送锁定与跟随 | ✅ 已修 |
| F8 | 刷新太频繁 | ✅ 已修 |
| F9 | 置顶分组、悬停卡住、底色粘连 | ✅ 已修 |
| F10 | 点击行展开回复并预填建议（浅灰、直接填入、缓存有有效期） | ✅ 已修 |
| F11 | 齿轮统一设置入口 | ✅ 已修 |
| F12 | 面板高度三档 | ✅ 已修 |
| F13 | 不在 tmux 的会话一键整理并移进 tmux | ✅ 已修 |
| F14 | 偶发测试：共用全局语言设置 | ✅ 已修 |
| F15 | 冷启动点链接面板不出现 | ✅ 已修 |
| F16 | 同一会话的历史卡片一起刷新状态 | ✅ 已修 |
| F17 | 小莫气泡多题时不能自己写 | ✅ 已修 |
| F18 | 会话图标上的加载圈漂到左下角 | ✅ 已修 |
| F19 | 会话归档：30 天不用自动归档、经 Moss 清理（T2.5） | ✅ 已修 · 0.3.1 |
| F20 | 插件的后台 `claude -p` 被当成会话收进列表 | ✅ 已修 · 0.3.1 |
| F21 | cc-switch 换供应商后要退出会话、重进 tmux（T2.6） | ✅ 已修 · 0.3.1 |
| F22 | Moss 确认按钮被截断、看不全要同意什么 | ✅ 已修 · 0.3.1 |
| F23 | 两个提问出现四条消息：改成一张实时「等你处理」卡 | ✅ 已修 · 0.3.1 |
| F24 | Moss 不跟随 API 供应商切换（0.3.0 回归） | ✅ 已修 · 0.3.1 |
| F25 | 找回失败没有任何提示：统一失败提示，可「交给 Moss 处理」；被顶替的副本安静关闭 | ✅ 已修 · 0.3.1 |
| F26 | 闲着但还在跑的会话没法手动归档：加「结束并归档」 | ✅ 已修 · 0.3.1 |
| F27 | 会话最后一句显示看不懂的 id（后台任务通知） | ✅ 已修 · 0.3.1 |
| F28 | 齿轮菜单按 Mac 习惯重排；关于页加 slogan、MIT 开源、网站与 GitHub | ✅ 已修 · 0.3.1 |
| F29 | 展开「已归档」后没滚到可见处 | ✅ 已修 · 0.3.1 |
| F30 | 「回到最新」改成朦胧条 + 灰色小字，悬停浅 badge | ✅ 已修 · 0.3.1 |
| F31 | 已结束的会话从归档拿回后在列表里找不到 | ✅ 已修 · 0.3.1 |
| F32 | 等你处理卡按钮位置突兀；小莫选项悬停不是手形 | ✅ 已修 · 0.3.1 |
| F33 | 点「关于」卡死（系统登录项服务卡住主线程） | ✅ 已修 · 0.3.1 |
| F34 | 中文输入法回车选词时把消息发出去了 | ✅ 已修 · 0.3.1 |
| F35 | 本机 App 整个卡死：系统 fseventsd 不回话时主线程卡在 FSEventStreamStart | ✅ 已修 · 0.3.1 |
| F36 | 同名会话在 App 里归档失败（App 按名字下命令，命令行分不清）：一律按会话 id | ✅ 已修 · 0.3.1 |
| F37 | Moss 找不到归档 / 30 天内关掉的会话：沙箱打不开注册表，`cc-moss ls` 失效 → 守护进程写脱敏的 `sessions.json`，Moss 的 `ls` 读它 | ✅ 已修 · 0.3.2（待发） |

## 推进节奏
- 第一波（并行）：T1.1 探针、T1.2 bypass、T2.1 守护进程
- 汇合点 1：T1.1 结论 → T1.3 开工；T1.3 合入 → T1.4；P1 四个都合入 → 发 0.3.0
- 汇合点 2：T2.3 合入后三路并行：T2.4、T3.1、T4.1

## 开放问题
- Q1 版本号：每个 Phase 一个小版本（0.3.0 起），P4 后是否直接 1.0？P4 收尾时定
- ~~Q2 Moss 默认名字与头像~~：默认「Moss」，头像为 ∞ 标志的品牌色圆角方块（T2.3 定）
- ~~Q3 代答默认等待时长~~：T1.1 实测后定为 300 秒（A5）

## 已定调（防翻案）
- D1 Slack 只在频道、不接私聊，频道内人人权限一样（兜底：允许频道、审计、暂停）
- D2 bypass 直接默认开启，升级不询问
- D3 Moss 一直在线，空闲不耗额度
- D4 先 bypass 与代答（P1 = MVP）
