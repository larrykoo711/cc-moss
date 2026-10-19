# S1 参考项目学习：模块拆解与移植评估

> 只读学习，没有运行参考项目的代码。下文的路径都相对参考项目根目录。
> 参考项目里的主技能目录和同前缀的 skill、环境变量、tmux 名都带项目名前缀，本文统一写成占位：
> 主技能记作 `<main-skill>`（位于 `agents/.template/.claude/skills/<main-skill>/`），
> 渠道 skill 记作 `<p>-channel`，密钥 skill 记作 `<p>-vault`，环境变量记作 `<P>_XXX`，tmux 名记作 `<p>-main` / `<p>-s-<id>`，入口文件记作 `packages/core/src/<p>-server.ts`。

## 0. 一句话结论

参考项目是 "TS 薄壳 + tmux 里常驻的交互式 `claude` 当大脑"：代码只管收消息、往 TUI 注入、读 transcript 拿回复、发出去；
调度、记忆、自升级都写进 prompt 和 skill，由 Claude 自己完成。它最值得搬的是**驱动协议**（注入、回读、Stop hook 哨兵、按时间戳认领回复）、
**"主会话只当耳朵和嘴巴"这条纪律和它的机制化手段**、**常驻 steward 巡检会话**和**主会话死亡后的阶梯重启**。
它**没有**真正的系统级守护进程配置（pm2/systemd 只停在文档里），这一块 cc-moss 要自己用 launchd 做。

## 1. 总体架构

### 1.1 组件

| 组件 | 位置 | 作用 |
|---|---|---|
| 入口进程 | `packages/core/src/<p>-server.ts` | 装配实例目录、单实例锁、日志、Supervisor、OutboxWatcher、命令菜单同步 |
| 实例装配 | `packages/core/src/instance.ts` | 实例目录里没有 `CLAUDE.md` 就从 `agents/.template` 整份复制；已存在就不覆盖（`ensureInstance`），老实例补 `steward/`（`ensureStewardHome`） |
| 单实例锁 | `packages/core/src/instance-lock.ts` | `<workdir>/.<p>.lock` pidfile + `kill 0` 判活，用 `wx` 原子创建；以 pidfile 为准，不信 tmux 名 |
| Driver | `packages/core/src/driver/tmux-brain-driver.ts`、`driver/types.ts` | 纯 tmux 驱动交互式 claude（1140 行，项目的"命脉"） |
| Gateway | `packages/core/src/gateway/telegram-inbound.ts`、`telegram-outbound.ts`、`chat-key.ts`、`types.ts` | Telegram 长轮询收消息、Bot API 发消息；`InboundSource` / `OutboundSink` 与平台无关（`gateway/types.ts:98,126`） |
| Supervisor | `packages/core/src/supervisor/supervisor.ts`（2376 行） | 队列、合并窗口、消息头、回复控制标记、迟到回复、主会话死亡探测与重启、steward 请求执行 |
| Steward | `packages/core/src/steward/steward-manager.ts` + `agents/.template/steward/` | 第二个常驻 claude 会话，代码层定时 tick，负责巡检和上报 |
| Outbox | `packages/core/src/outbox/outbox-watcher.ts` | 主会话把文件放进 `outbox/` 并追加 manifest 行，由持有 token 的进程代发 |
| Loading 阶梯 | `packages/core/src/loading/*` | 1s 表情回应，10s 起文字进度，8 级（`loading/ladder.ts:14`） |
| 配额治理 | `packages/core/src/quota-governance.ts`、`usage-probe.ts` | 往空闲的主会话注入 `/usage` 抓面板，判断是否撞配额（不消耗模型轮次） |
| 计划中 | `packages/a2a-sidecar/`、`registry/`、`packages/proxy/`、`packages/shared/` | 只有 README，没有代码 |

### 1.2 进程模型

- 一个 Node 进程（入口进程）对应一个"数字人"。它在 tmux 里拉起两个常驻 claude：`<p>-main`（主会话，cwd = 实例根）和 steward（cwd = `steward/`）。
  见 `<p>-server.ts` 的 `MAIN_TMUX_NAME` 和 `supervisor.ts:436` 起的 `start()`。
- 主会话派活时用 `scripts/dispatch.sh` 再起 `<p>-s-<id>` 子会话，每个子会话是独立 tmux + 独立进程（`agents/.template/scripts/dispatch.sh`）。
- 隔离靠部署边界（整机、低权限用户、容器、VM），应用内不设围栏（`deploy/README.md`、`docs/architecture.md` "Isolation" 行）。

### 1.3 数据流（`README.md` "How a message flows"、`docs/architecture.md` "Data flow"）

```
Telegram → 长轮询 → 归一化 chatKey=platform:channelId:chatId（gateway/chat-key.ts）
 → supervisor：立即回执（loading 阶梯）+ 原样注入（不拦截命令、不做意图判断）
 → driver：tmux set-buffer / paste-buffer -p 注入
 → 主会话思考 → 实例自带的 Stop hook 写 <workdir>/.done.sentinel + events.jsonl
 → driver 从 transcript JSONL 读出属于本轮的干净回复
 → supervisor 解析 [[...]] 控制标记 → Bot API 发回
```

### 1.4 "代码是耳朵和嘴巴，AI 是大脑"

- 原文出处：`README.md` 开头、`docs/architecture.md` "One sentence"、`agents/README.md` "Design principles"。
- 落到代码上：Supervisor 头注释明确写了"不做意图判断、不做路由"（`supervisor.ts:1-12`）；`/clear`、`/compact` 是仅有的直通原生命令（`supervisor.ts:79`）。
- 落到 prompt 上：不用 SQLite（用 `sessions/SESSIONS.md`、`jobs/SCHEDULE.md` 文本表），不写 cron 代码（用原生 `/schedule`、`/loop`），
  不自建记忆（用原生 `/remember`），装 skill 让 Claude 自己跑 `npx skills add`（`agents/README.md` "Design principles"）。
- **但后来的轮次修正了这条原则**：凡是依赖"LLM 自觉"的都失败了，又被一项项改成代码机制。
  例如心跳靠主会话自己 `/loop`，结果 5 天一次都没跑，于是改成代码层 tick 驱动 steward（`specs/round-011/00-steward-architecture.brainstorm.md` "Problem"）；
  会话状态靠主会话回写，11 行全卡在 running，于是改成 Stop hook 自动回写（`agents/.template/hooks/stop-sentinel.sh` 第 3 段）。
  项目里把这条规律叫"文档约束不如机制"。**对 cc-moss 的启发：判断留给 AI，节拍、存活、状态回写必须是代码。**

## 2. 大脑怎么跑

### 2.1 启动方式

- 用**交互式 TUI**，不是 `-p`，也不是 stream-json、node-pty 或 tmux control-mode。理由写在 `driver/types.ts` 头注释、`CLAUDE.md` 末行：`-p` 跑一次就退出，而这里要能一轮接一轮地注入。
- 启动命令（`tmux-brain-driver.ts:294-315`）：
  `tmux new-session -d -s <name> -c <workdir> -e ZDOTDIR=<空目录> "claude --session-id <自选UUID> [--model m] --strict-mcp-config --mcp-config <实例.mcp.json或空配置> --dangerously-skip-permissions"`
  - `--session-id` 预先指定 UUID，transcript 路径就能直接算出来：`~/.claude/projects/<cwd slug>/<uuid>.jsonl`，slug 规则是非 `[A-Za-z0-9-]` 字符全换成 `-`，先 realpath（`:82-96`）。
  - `--strict-mcp-config` 加一个空配置，避免继承用户全局 MCP，也避开 bypass 模式关不掉的项目 MCP 审批弹窗（`:275-292`）。
  - `ZDOTDIR` 指向空目录，跳过 `~/.zshrc`，免得插件输出污染 capture-pane 回读（`:302-315`）。
  - 全局 `tmux set -g exit-empty off`、`destroy-unattached off`（`:269-273`）。
  - 就绪检测：轮询 capture-pane，遇到 "Trust folder" 就按回车，看到 `❯` 且连续 3 次画面不变才算就绪（`:903-947`）。子会话还要处理 "external imports" 弹窗（`dispatch.sh` 第 6 步）。
- **不用 `--settings`**：它在 hooks 这一层是整键覆盖而不是合并，会把实例自己的 PreToolUse 围栏冲掉，所以 hook 全放进实例目录的 `.claude/settings.json`（`docs/architecture.md` "Instance assembly"、`tmux-brain-driver.ts:263-265`）。

### 2.2 注入与认领回复（`tmux-brain-driver.ts:354-521`）

1. **注入**：`set-buffer -b <会话专用buffer> -- msg` → `paste-buffer -p -d`（bracketed paste，多行不会被拆成多次提交）→ 等 300ms，capture-pane 回读，找消息末尾 12 个字符 → 回车只用 `send-keys Enter`。
   回读失败重贴一次；还失败就发 Esc 恢复后再贴一次；仍失败就抛错（`:524-548`）。**正文永远不走 send-keys。**
2. **认领**：记下注入时刻 tAnchor → 在 transcript 里找时间戳晚于 tAnchor、且含末尾片段的 user 条目（找不到就当被吞了，重注一次）
   → 等 Stop hook 哨兵 → 取该 user 条目之后、**时间戳最大**、非 sidechain、`stop_reason` 为 `end_turn`/`stop_sequence` 的 assistant 文本（`:606-631`）。
   transcript 的行序和时间序会颠倒，所以按时间戳取，不按文件位置取。
3. **哨兵和 transcript 落盘有竞态**：哨兵到了先轮询 3 秒确认；没有属于本轮的回复就当残留哨兵，删掉继续等（`:454-463`）。
4. **`/clear` 会换 transcript 文件**：文件不再增长时扫描 slug 目录，切到更新的 jsonl（`:652-684`）。
5. **超时**（默认 300s）：先看画面有没有 "esc to interrupt" 这类运行中标记。还在运行就不打断，挂一个迟到观察器（15 分钟、带代际令牌）；
   画面上是 "Do you want to proceed / ❯ 1." 这种弹窗就按可打断处理，发 Esc 恢复（`:154-175`、`:465-521`、`:726-768`）。
6. **"转后台"识别**：本轮调用过 `ScheduleWakeup`，或者 Bash 跑过 `scripts/dispatch.sh` / `relay.sh`，就算异步，不报超时（`:135-204`）。
7. **命令轮**（`/clear`、`/compact`）：不走认领协议，画面静止或哨兵出现就算完成（`:416-422`、`:686-714`）。
8. 同一会话的 send 串行排队；`probeUsage` 注入 `/usage` 抓面板后必发 Esc（`:826-880`）。

### 2.3 权限模式

主会话、子会话、steward 全部用 `--dangerously-skip-permissions`。安全靠 `permissions.deny` 加 PreToolUse hook 托底
（deny 规则在 bypass 下照样生效，见 `agents/.template/.claude/settings.json` 的 `//` 注释）。

### 2.4 Hook 的用法（`agents/.template/.claude/settings.json`）

| Hook | 脚本 | 作用 |
|---|---|---|
| Stop | `hooks/stop-sentinel.sh` | ① touch `$CLAUDE_PROJECT_DIR/.done.sentinel`（用 `CLAUDE_PROJECT_DIR`，不用 pwd，避免 cwd 漂移）；② 父目录是 `sessions`/`jobs` 时往 `events.jsonl` 追加一行；③ 解析 `result.md` 的 Status，调 `scripts/ledger-update.sh` 回写台账 |
| PreToolUse: AskUserQuestion | `hooks/pretooluse-ask-block.sh` | 硬拦，并在 reason 里引导改用 `[[buttons:]]`（渠道用户看不到终端里的选择框） |
| PreToolUse: Bash | `hooks/pretooluse-blacklist.sh` + `hooks/main-harness.sh` | 危险命令黑名单（`rm -rf /`、`curl|sh`、读凭据）；骨架保护；"nudge"：allow 加 `additionalContext`，把重活引向派发 |
| PreToolUse: Write/Edit | `main-harness.sh` + `hooks/pretooluse-soul-snapshot.sh` | 禁止改 settings/hooks、禁止清空身份文件、禁止主会话手写交付物；写身份文件前先快照到 `.snapshots/` |

deny 列表：`AskUserQuestion, Agent, Workflow, SendMessage, WebFetch, WebSearch` 加两个浏览器 MCP。
原因是主会话用了进程内子代理，一轮跑了 1 小时 15 分，彻底听不到新消息（`settings.json` `//orchestration` 注释、`<main-skill>/SKILL.md:18`）。

### 2.5 恢复与重启

- 主会话**重启时全新启动，不 `--resume`**：身份都在文件里，崩溃前的 TUI 状态不可信。重启后注入一条"你刚重启过，去读 SESSIONS.md / SCHEDULE.md 恢复工作"的 boot 消息
  （`specs/round-017/03-main-death-resilience.brainstorm.md` "Core insight"、`supervisor.ts:825-860`）。
- 子会话**要 resume**：`scripts/reopen.sh` 在原目录跑 `claude --resume <.session-id>` 续上上下文；目录或 id 缺失就大声失败，不悄悄新建（`reopen.sh` 头注释）。
- Telegram offset 持久化到 `.tg-offset`，重启后补收停机期间的消息（`supervisor.ts:405-409`）。

## 3. 主会话（Main Agent）

- **角色**："数字人唯一的耳朵和嘴巴"，要秒级响应，自己不干重活（`<main-skill>/SKILL.md:12-24`）。
- **人设文件**：`agents/.template/CLAUDE.md` 用 `@` 依次引入 SECURITY、IDENTITY、SOUL、USER、TOOLS、TEAMS 和运行时数据 `sessions/SESSIONS.md`、`jobs/SCHEDULE.md`。
- **固定任务范围**（ROUTER 分级，`SKILL.md:52-67`）：L0 寒暄（可以 `[[silent]]` 或 `[[react]]`）/ L1 60 秒内能查完的直接做 / L2 超过 60 秒或有交付物的必须派发 / L3 复杂工程先派人拆解。
  另有一条硬规则："**产出是文件就派发**"，main-harness 直接拦主会话写交付物（`SKILL.md:69-76`）。
- **派发**：`echo "<任务>" | scripts/dispatch.sh <同事名> <slug> [来源chat] [用户]`（`dispatch.sh`）：
  查 TEAMS.md 拿 MBTI、专长、默认模型 → 同一个同事同时只接一个活（`--force` 可破例）→ 复制 `sessions/.session-template` → 写 `context.md`（任务 + 来源 + 人设卡）
  → 冷启动互斥（mkdir 自旋锁）→ 起 tmux + claude → 就绪等待（90s）→ 注入带唯一标记 `<P>K<sid>` 的开工语并回读确认 → 在 `SESSIONS.md` 追加 10 列一行。
- **后续操作**：`relay.sh append|interrupt <id>`（追加，或先 Esc 再纠偏）；`reopen.sh <id>` 复活；`ledger-check.sh --task-line` 判断该续还是该新开
  （`<main-skill>/references/sessions.md` "Send a follow-up"、"After delivery"）。
- **派到哪个目录**：参考项目**不选目录**。子会话一律在 `sessions/<id>/`，按 inbox/workspace/outbox 三区工作（`sessions/.session-template/CLAUDE.md`）。
  **cc-moss 的"自己判断目录、拿不准就问"在参考项目里找不到对应，要自己设计。**
- **子会话协议**：`result.md` 写 `DONE / NEEDS_INPUT / NEEDS_GRANT / ERROR`，Stop hook 自动回写台账；越界权限走 `sessions/grants/<id>.json`，只有主会话能写，
  由 session-harness 注入环境变量（`hooks/session-harness.sh` 头注释）。
- **调度**：原生 `/schedule` 加 `jobs/` 目录（每个 job 一个家，含 LOGGING.md、state.md）（`references/schedule.md`、`jobs/CLAUDE.md`）。
- **记忆**：原生 `/remember`；人放进 `contacts/` 通讯录（`references/memory.md`、`references/contacts.md`）。
- **自升级**：`npx skills add`；改 hook 或 SOUL 前先快照，改完写 `dreams/` 说明原因（`references/self-upgrade.md`）。
  后来 main-harness 又禁止主会话改 settings、hooks 和 dispatch.sh，工具坏了只上报不自修（`references/sessions.md` "When dispatch fails"）。
- **会话打理**：`references/self-check.md` 列了僵尸、卡住、可清理、漏跑、堆积几类，"拿不准就留着或问人"。这和 cc-moss 的 Doctor 几乎一一对应。

## 4. 守护与监管

- **系统级守护：基本没有。** README 写 "pm2 (+ systemd fallback)"（`README.md:105`、`packages/core/src/supervisor/README.md`），但仓库里没有任何 pm2、systemd、launchd 配置，
  `deploy/` 只有 README。入口进程自己挂掉没人拉起，只靠单实例锁防重入，启动时清理残留的 `<p>-main`（`<p>-server.ts` "dead-shell"段）。
- **进程内监管（已实现）**：
  - 主会话死亡探测：每 60s 跑一次 `tmux has-session`，每条入站消息前也查一次。只有 "can't find session" / "no server running" 才判死，其它错误一律当活着（宁可漏判也不误杀，`tmux-brain-driver.ts:784-812`）。
  - 重启阶梯：30s / 2m / 10m，之后每小时一次，没有上限；重启前再探一次，以防它自己活过来（`supervisor.ts:386-391`、`:719-800`）。
  - 主会话死着时来消息：代码层直接回一句本地化的"正在重启"，消息留在队列里等（brainstorm 决策 4）。用户告警每天最多 3 条。
  - 入站长轮询死了：每 30 分钟自愈重试，发恢复通知（`supervisor.ts:550-640`）。
  - steward 看门狗：tick 注入失败或 tmux 死了就重启 steward，连续失败 3 次先做最后一次重启，再失败才发 `steward_fatal`（`steward-manager.ts` 头注释）。
- **Steward**（`agents/.template/steward/CLAUDE.md`）：每 5 分钟 tick 一次，忙的时候跳过不排队；跑 `scripts/steward-scan.sh`，输出一个 JSON，状态有 dead/api-error/interrupted/stuck/working/delivered/idle/limit-stalled，
  stuck 要连续两次看到才算 → 回写台账 → 归档 → 按阶梯上报。它的手只有请求文件 `steward/requests/*.json`（`notify_main` / `compact_main` / `notify_user`），
  由 supervisor 去重、冷却后执行；它不碰别人的 pane，不碰 token；每 24 个 tick `/clear` 一次，保证"steward 不需要 steward"。主会话上下文到 70% 时申请 `compact_main`。
- **"打不死"的含义**：不是进程不会死，而是**死了会被发现、被拉起、被如实告知**。身份在文件里，所以全新重启是安全的。
- **日志**：`<P>_DEBUG=1` 时写 `<workdir>/logs/`（文件名带启动时刻，重启不互相覆盖），事件流 `logs/events.jsonl` 落盘前先脱敏（`<p>-server.ts`、`redact-secrets.ts`）。

## 5. 渠道

- **抽象**：`InboundSource.start(onMessage)` / `OutboundSink.send|sendMedia`，`ChannelMessage` 与平台无关（`gateway/types.ts`）。地址树 provider → channel → chat，键由 `chat-key.ts` 统一构造和解析（`docs/architecture.md` "Addressing tree"）。
- **入站**：自己实现 getUpdates 长轮询，offset 持久化；409 冲突（两个实例抢同一个 token）先给宽限期，持续冲突才致命退出；附件自动下载进 `inbox/`（`telegram-inbound.ts`）。
- **消息头**：`[from @user | telegram/<bot> | chat_id=… | button click | 2026-06-11T06:30Z]` 加附件行（`supervisor.ts:1612-1638`）。主会话靠消息头知道是谁、从哪来。
- **回复控制标记**（`supervisor/reply-controls.ts`）：`[[silent]]`、`[[react:👍]]`、`[[more]]`（10 秒内没有新消息就合成一轮"继续说"，最多 3 次）、
  `[[buttons: A | B]]`（最多 6 个）、`[[say:<chatKey>]]`（内部轮主动投递的唯一出口，必须是 knownChats 里的 chat）。代码块里的标记不识别。
- **按钮**：outbound 生成 `callback_data=btn:<index>`，每行 3 个（`telegram-outbound.ts:89-94`）；点击时先立刻 ack，反查文字，转成 `via=button` 的普通消息入队，再把原按钮改成 ✅ 已选（`telegram-inbound.ts:481-491`）。
- **主动发送防泄漏**：outbox 只发给入站历史里出现过的 chat（`.known-chats.json`）；文件 realpath 必须落在 `outbox/` 下（`outbox-watcher.ts` 头注释）。
- **体验层**：合并窗口 1.5s（一句话加一张截图合成一轮）、每个 chat 最多排 20 条、loading 8 级阶梯、语言按 chat 记忆（`.chat-language.json`、`/language`）。
- **按渠道分人设**：没有。人设是整个实例共用一份，渠道差异只在 `<p>-channel` skill 的能力矩阵里。
- **A2A**：只有计划（`packages/a2a-sidecar/README.md`、`registry/README.md`），没有代码。
- **Slack**：只在 `gateway/README.md` 提过"将来经 cc-connect 接入"，没有任何实现。

## 6. 人设

- **模板**：`agents/.template/` 整份复制就是一个数字人（`agents/README.md` 目录树）。
- **soul-file 矩阵**：SECURITY（最高优先级）> SOUL（MBTI 底座固定，加说话方式）> USER（团队偏好）> 当下任务（`SOUL.md` "Priority order"）。
  IDENTITY 管名字和 emoji，TOOLS 是本机工具笔记，HEARTBEAT 是空闲清单，TEAMS 是 10 名带 MBTI、专长、默认模型的虚构同事。
- **组装方式**：只靠 Claude Code 原生的 CLAUDE.md `@` 引入 + 从 cwd 往上自动加载 `.claude/skills`，没有旁路注入（`docs/architecture.md` "Workspace boundary"）。
  子会话的 CLAUDE.md 自成一体，不继承主会话的 SOUL，人设卡由 dispatch 写进 `context.md`。
- **改人设的闸门**：SOUL/IDENTITY/USER 只能通过 `/profile` 命令改，普通消息要求改一律拒绝，防止社工篡改（`SOUL.md` "My boundaries"）。
- **说话纪律**很值得抄："聊天就聊天，别写文档"、对方写多长就回多长、不暴露路径和内部机制、说用户的语言且不中英夹杂（`SOUL.md` "How I speak"、"Adaptive language"）。

## 7. 安全与密钥

- token 只在入口进程的环境变量里（`TELEGRAM_BOT_TOKEN`），**从不进实例目录，也不进 agent 的手**；agent 发文件走 outbox，steward 发消息走请求文件（`outbox-watcher.ts` 头注释、steward brainstorm Axis 3）。
- 渠道里收到的密钥用 `<p>-vault`（基于 age 和 SSH 密钥的本地加密库）入库；只回打码形式，提醒用户撤回原消息；
  使用时只在命令替换里 `--reveal`（`<main-skill>/references/secret.md`、`<p>-vault/SKILL.md`）。
- 事件流落盘前脱敏（`packages/core/src/redact-secrets.ts`）。transcript 里仍可能留有明文，项目自己承认这一点（`secret.md` "Where a pasted secret can persist"）。
- `SECURITY.md`：漏洞私下报告；密钥文件不进仓库；明文只能流入渠道，不能流出。
- 项目自己的判断：Bash 字符串黑名单只防手滑，挡不住 `python -c`，真正的边界是 OS 沙箱（`agents/README.md` "Session isolation"）。

## 8. 许可证

- `LICENSE` 是 **MIT**。允许复制和修改，条件是在副本里保留原版权声明和许可文本。
- 建议：cc-moss 是 Rust + Swift，TS 代码本来就搬不过来，**以重新实现为主**，引用参考项目的设计思路即可。
  prompt、skill、hook 脚本（SOUL 的说话纪律、steward 清单、`stop-sentinel.sh`、`pretooluse-ask-block.sh`）如果要**大段照搬原文**，
  就在 cc-moss 对应目录放一份 MIT 声明，写明出处。本仓库是公开仓，按 `.sdd` 规则，出处只写"参考项目（MIT）"，不写项目名和组织名；
  这和 MIT 要求保留原版权行有冲突，所以**倾向改写而不是照搬**，绕开署名问题。
- 子会话模板里附带的第三方 skill 各有各的 LICENSE（如 `sessions/.session-template/.claude/skills/archify/LICENSE`），不要连带复制。

## 9. 移植表

| 参考模块 | 做什么 | 移植到 cc-moss | 量级 | 风险 |
|---|---|---|---|---|
| tmux 注入（`tmux-brain-driver.ts:523-557`） | paste-buffer -p、回读确认、回车 | Rust CLI：扩展现有 `crates/cc-moss/src/cmd/send.rs`（已经用 `paste-buffer -p`） | S | 不同 Claude Code 版本的提示符和弹窗文案会变 |
| 回复认领（`:559-650`） | tAnchor、user 条目、按最大时间戳取 end_turn | Rust CLI：`moss-core/src/transcript.rs` 加反向扫描 | M | transcript 格式没有文档，`/clear` 会换文件 |
| Stop 哨兵（`hooks/stop-sentinel.sh`） | 完成信号 + events | Rust CLI：cc-moss 已经注册 hooks，Stop 事件直接写状态目录，不要再写 shell 脚本 | S | Stop 和 transcript 落盘有竞态，需要 3 秒确认窗口 |
| 就绪检测 + 信任弹窗（`:899-947`、`dispatch.sh` 第 6 步） | 起 TUI 后等 `❯` | Rust CLI：`cc-moss new` 加 `--wait-ready` | S | 弹窗文案随版本变化，要集中维护 |
| 超时、迟到观察器、运行中标记（`:465-521`、`:726-768`） | 长任务不误杀 | Rust CLI（在 daemon 里） | M | 实现复杂，先做"运行中就不打断"这一条 |
| 主会话死亡探测 + 重启阶梯（`supervisor.ts:650-860`） | 打不死 | Rust daemon + launchd LaunchAgent（`KeepAlive`）负责拉起 daemon | M | 判活读错了必须当活着，不能误杀 |
| 系统级守护（参考项目缺失） | 入口进程自身保活 | Swift App 的 `LoginItem.swift` 或 launchd plist，由 `cc-moss install` 安装 | M | 两个守护者打架：App 和 launchd 都去拉起时要有锁 |
| 单实例锁（`instance-lock.ts`） | 防止双实例 | Rust：pidfile + `kill 0`（cc-moss 已有 `write_lock`） | S | 低 |
| Steward（`steward-manager.ts` + `steward/CLAUDE.md` + `steward-scan.sh`） | 定时巡检、上报 | 先做 Rust 定时器 + 扫描 JSON（`cc-moss ls --json` 的扩展）；判断交给 Moss 会话。**要不要第二个常驻 LLM 会话可以先不定**，Doctor 按需触发可能就够了 | M | 常驻两个会话会翻倍消耗配额 |
| self-check / Doctor 清单（`references/self-check.md`） | 僵尸、卡住、清理 | prompt + skill 文件：Moss 的 `/doctor` 命令 | S | "拿不准就问"必须落到 UI 上，不能让它默默删 |
| ROUTER 分级 + "产出是文件就派发" | 主会话不干重活 | prompt + skill，加 PreToolUse hook 拦写交付物和 deny 列表 | S | 只写文档约束会失效，必须配 hook |
| dispatch.sh | 起子会话 | Rust CLI：`cc-moss new --bypass --dir <d> --prompt-file`，由 Moss 调用；台账用 cc-moss registry，不用 SESSIONS.md | M | 选目录的逻辑参考项目里没有，要自己设计（候选列表加询问） |
| relay / reopen | 追加、纠偏、复活 | Rust CLI：`cc-moss send`（已有）、`cc-moss restore`（已有），补 `--interrupt` | S | 低 |
| AskUserQuestion 拦截（`pretooluse-ask-block.sh`） | 选择框改到渠道 | 不能照抄：cc-moss 的 R3 是**代答**，不是禁用。PreToolUse 捕获问题，转给桌宠或 Slack 渲染选项，再把答案回填 | M | 回填的方式（hook 返回值还是注入按键）要先在 `claude-code-facts.md` 验证 |
| 回复控制标记（`reply-controls.ts`） | silent/react/more/buttons/say | Rust 解析器 + Swift 气泡和 Slack Block Kit 渲染 | S | 标记语法要和人设 prompt 同步维护 |
| Gateway 抽象 + Telegram | 渠道收发 | Rust：`Channel` trait；Slack 用 Socket Mode（不需要公网回调）；按钮用 Block Kit actions | L | Slack 签名和重连、多 workspace；token 只放 daemon 里 |
| outbox + knownChats | 文件投递、防泄漏 | Rust daemon | S | 低 |
| loading 阶梯 | 等待反馈 | Swift 宠物动画 + Slack reaction | S | 低 |
| soul-file 矩阵 | 人设组装 | prompt 文件：`~/.cc-moss/moss/` 下放 CLAUDE.md、SOUL、IDENTITY、USER；按渠道覆盖（参考项目没有，cc-moss 自己加） | S | 人设和"Mo"桌宠形象要保持一致 |
| 配额探测 `/usage` | 撞限检测 | 暂不需要（可以以后从 statusline 拿） | — | 往 TUI 注入命令有副作用 |
| 密钥库、脱敏 | 渠道密钥 | 先用 macOS Keychain 存 Slack token；其余暂不需要 | S | Slack 消息进 transcript 会留明文 |
| A2A、registry、proxy | 集群 | 不需要 | — | — |

## 10. 给 DESIGN 的输入

1. 驱动选型直接沿用"交互式 TUI + tmux + Stop hook + transcript 认领"，cc-moss 已经具备一半零件（hooks、send、transcript、registry）。
2. Moss 会话本身也是一个 cc-moss 托管会话，可以像普通会话一样被 restore；重启时按参考项目的做法全新启动，再注入 boot 消息，不 resume。
3. 节拍、存活、状态回写全部放在 Rust daemon 里；Moss 只负责判断和说话。
4. 参考项目里最容易翻车的地方：主会话自己干活导致听不到消息、`AskUserQuestion` 卡死无人值守的 TUI、MCP 审批弹窗、`~/.zshrc` 污染回读、
   Stop hook 用 pwd 定位、超时时误杀还在跑的工具。cc-moss 要逐条设计防线。
