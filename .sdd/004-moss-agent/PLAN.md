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
| P1 | 放手干 + 替你接住问题（MVP） | T1.1–T1.4 | 0.3.0 | 新会话不再卡权限；选择题不用进会话就能答 |
| P2 | Moss 住进来 | T2.1–T2.4 | 0.4.0 | 常驻守护、能和 Moss 对话、一键体检 |
| P3 | Moss 派活 | T3.1 | 0.5.0 | Moss 自己挑目录起 bypass 会话、做完回报 |
| P4 | Slack 与路由 | T4.1–T4.2 | 0.6.0 | 团队在频道里用 Moss；人不在时问题转到 Slack |
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
| **T1.3** 代答中枢（CLI） | cc-moss | T1.1 | 🟦 Ready | todo/T1.3-ask-broker.spec.md | 假 claude 问答闭环、超时走原生 |
| **T1.4** 桌面答题 | cc-moss | T1.3 | ⬜ Backlog | — | 面板答完会话继续 |
| **T2.1** 守护进程 | cc-moss | — | 🟨 In Progress | in-progress/T2.1-daemon.spec.md | kill -9 后自动回来 |
| **T2.2** Moss 常驻 | cc-moss | T2.1 | ⬜ Backlog | — | 杀掉后阶梯恢复、列表里结束不了 |
| **T2.3** 和 Moss 对话 | cc-moss | T2.2 | ⬜ Backlog | — | 面板发一句、看到回复 |
| **T2.4** Doctor | cc-moss | T2.3,T1.3 | ⬜ Backlog | — | 体检报告正确、关闭先问 |
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

## 推进节奏
- 第一波（并行）：T1.1 探针、T1.2 bypass、T2.1 守护进程
- 汇合点 1：T1.1 结论 → T1.3 开工；T1.3 合入 → T1.4；P1 四个都合入 → 发 0.3.0
- 汇合点 2：T2.3 合入后三路并行：T2.4、T3.1、T4.1

## 开放问题
- Q1 版本号：每个 Phase 一个小版本（0.3.0 起），P4 后是否直接 1.0？P4 收尾时定
- Q2 Moss 默认名字与头像：默认「Moss」，头像沿用 ∞ 标志的变体？T2.2 spec 时给方案
- ~~Q3 代答默认等待时长~~：T1.1 实测后定为 300 秒（A5）

## 已定调（防翻案）
- D1 Slack 只在频道、不接私聊，频道内人人权限一样（兜底：允许频道、审计、暂停）
- D2 bypass 直接默认开启，升级不询问
- D3 Moss 一直在线，空闲不耗额度
- D4 先 bypass 与代答（P1 = MVP）
