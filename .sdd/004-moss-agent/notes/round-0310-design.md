# Round 0.3.10 现状与设计（2026-11-09）

立项依据见 `round-0310-drivers.md`。锚点均于 2026-11-09 在 `3c239f3`（main）上实读。

## 现状（✅ 生效 / ⚠️ 部分 / ❌ 没有）

### F59 Moss 卡在 bypass 警告框
- ❌ 跳过警告的键写进了项目级：`crates/cc-moss/src/cmd/moss.rs:248-250` 把 `accept_bypass_prompt` 加到 `.claude/settings.json`，Claude Code 对这个键只认 user / local / managed（`notes/claude-code-facts.md:10`）。
- ⚠️ 本地级 `.claude/settings.local.json` 由 `provider::moss_settings`（`crates/moss-core/src/provider.rs:388-418`）生成，只放 `env` 和 `apiKeyHelper`；`home::lay_out` 会删掉 `.claude/` 下不归它管的文件（`crates/moss-core/src/moss/home.rs:116-126`），所以只能在 `moss_settings` 里加。
- ⚠️ 注释与实现不符：`moss.rs:245` 写「`--setting-sources project`」，实际是 `project,local`（`moss.rs:1701`）；`moss.rs:1601-1605` 说「用户设置照样加载」，不对。
- ⚠️ 只自动处理「信任此文件夹」：`accept_trust`（`moss.rs:1272-1292`，匹配 `reply.rs:329-350`）；bypass 警告、登录界面都没有识别。
- ❌ 卡住时状态报 `idle`：没有钩子、没有对话记录，`activity` 落到默认值（`crates/moss-core/src/moss/mod.rs:173-187`）；`ready_to_send` 永远不成立（`moss_talk.rs:358-364, 960-962`）；App 在 `idle` 时把排队消息标成「发送中…」（`apps/macos/Sources/CCMoss/MossPage.swift:538-540`）。
- ❌ 没有测试检查 Moss 这个键落在哪；`doctor` 不查 Moss 家的设置。
- ✅ 普通新会话不受影响：`setup` 把键写进用户级（`crates/cc-moss/src/cmd/setup.rs:91-104, 322-328`），有测试（`crates/cc-moss/tests/setup_cli.rs:526-570`）。

### F60 积压时你的消息排在系统动态后面（含 F63）
- ❌ 你的消息和动态同一条队列，严格按时间先后，一次一条（`crates/moss-core/src/moss/inbox.rs:48-53, 95-112, 719-729`），没有优先级。
- ⚠️ 动态只在进程内存里去重 10 分钟（`digest.rs:57, 100-114`）；写入前不查收件箱里还没送出的同类（`moss_talk.rs:846-885`）。
- ❌ 不看 `at`，没有过期，也没有容量上限（`inbox.rs:679-692`）。
- ❌ 送进 Moss 的只有正文，不带时间（`inbox.rs:710-716`；动态正文 `digest.rs:163-170`）→ F63：Moss 把几天前的「做完了」当成今天。
- ⚠️ 每日体检每天最多一次，但只看 Moss 进程在不在（`moss.rs:1197-1237`），不看收件箱里是否已有一条没送出的体检；卡住三天就攒下三四条，解卡后连做。
- ✅ `facts.json` 的闲置时间正确（`checkup.rs:123-127`，Stop 钩子更新 `last_active_at`：`registry.rs:463-473`），本机逐个核对一致。

### F61 分出来的会话和原会话同名
- ✅ 新会话编号总是新建一行（`registry.rs:316-336, 388-397`）。
- ⚠️ 名字来自对话记录里最后一条 `custom-title`（`hook.rs:186-188, 247-265`；`transcript.rs:205-218`）；Claude Code 2.1.319 分叉时**用新编号**把原名写进新记录（本机实测：分叉记录 1509 行里 35 条 `custom-title`，`sessionId` 都是新编号，标题都是原名）。所以不能靠「编号不同就不认」来识别继承。
- ❌ 登记表的名字从不去重：`rename()`（`registry.rs:1638-1665`）、钩子新建行（`registry.rs:416-418`）、补名（`registry.rs:704-739`）都可以写出与其他开着的行同名；`-2/-3` 只用在 `new`（`cmd/new.rs:37-50`）、自动起名的 adopt 和 tmux 名（`naming.rs:144-153`）。
- 复发：F41（`--fork-session` 进程身份）、F57（后台进程接手后一段对话两行）同族。

### F62 没说过话就退出的会话被列成「断了」
- ⚠️ 只有一条路径会把「从没收到过消息」的行关掉：`release_process`（`registry.rs:1365, 1381-1388`）；异常原因的 SessionEnd（`registry.rs:515-524`）和进程消失（`slot.rs:120-137`、`reconcile.rs:172-181`）都留成「断了」。
- ❌ 列表不过滤：`cc-moss ls`（`cmd/ls.rs:28, 97-102`）、App（`MossKit/Filter.swift:44-48`、`Restore.swift:86-87`）都把它当可找回；「没什么可找回的」要等找回失败才出现（`Restore.swift:140-141`）。
- ⚠️ `last_prompt` 只由 Prompt 钩子填；adopt 建的行可能说过话却没有它（`registry.rs:452-456`），判断时要看对话记录（`transcript::last_prompt`，`transcript.rs:223`）。

## 假设（请纠偏）

- A1 Moss 的 bypass 是 cc-moss 自己的设计（R1/T1.2 已拍板）：把键写进 Moss 的本地级设置，**一律写**，不看用户级设置（用户 2026-11-09 选「一律写键，卡住也只报告」）。
- A2 万一 Moss 仍停在任何确认框上（bypass、登录、以后新出的），cc-moss 不替用户点「同意」，只识别、如实报告，并给出处理办法。
- A3 你的消息优先于系统动态；动态过时就不再送（按会话合并，只留最新一条；超过 6 小时的丢掉）；送出的动态带上发生时间（超过 15 分钟时）。
- A4 分叉或后台接手产生的新会话，名字与另一个还开着的会话重名时，自动加 `-2`、`-3`，和 `cc-moss new` 一样；之后补名不会改回去。原会话不动。
- A5 从没说过话就退出的会话直接关掉（不归档、不删文件），各条退出路径都一样；升级后由对账把现有的这类行一并关掉。adopt 建的行先看对话记录，确实一句没有才关。

## 方案

| # | 任务 | 改动面 | 验收要点 |
|---|------|--------|----------|
| T2.20 | F59a 键写进 Moss 本地级设置 | `provider::moss_settings`、`moss.rs` 注释、`doctor` 查 Moss 家 | 临时 HOME：`lay_out` + `sync_provider` 后 `settings.local.json` 有键；用户级明确 `false` 时没有；隔离 tmux 起真 Claude Code 不出现警告框（探针，记 `reports/`） |
| T2.21 | F59b 卡住时如实报告 | 守护进程窗格识别（复用 `accept_trust` 的轮询）、`activity: "blocked"` + 原因、App 排队消息标签、`moss status` 文案 | 伪造窗格文本的单测；状态 JSON 新值有测试；App 测试覆盖新标签；不发送任何按键 |
| T2.22 | F60 收件箱：你的先送、动态去旧并带时间、体检不重复 | `inbox.rs` 取件顺序与过期、`digest`/`tell` 写入前合并、送出时加时间、`daily_checkup` 查待送 | 单测覆盖：你的消息插队、同会话动态合并、超 6 小时丢弃、时间前缀、已有待送体检时不再加 |
| T2.23 | F61 同名自动加编号 | `registry` 新建行与补名走 `naming::unique`（对照开着的行） | 单测：分叉行得 `-2`；补名不改回；同编号的续接不受影响 |
| T2.24 | F62 没说过话的会话退出即关 | SessionEnd 异常、进程消失、对账三处统一规则；adopt 行查对话记录 | 单测覆盖三条路径；对账关掉现有的这类行；有过对话的行不受影响 |

- 版本：0.3.10；分支 `feature/004-moss-stuck`；每个任务一个 commit，收尾 squash。
- 验证：L1 各任务过滤测试；L2 `cargo clippy --all-targets -- -D warnings && cargo test --all`；App `apps/macos/scripts/swift.sh test`；发版前 L3 + Linux 全量。
- 端到端只用独立 tmux socket 与临时 HOME；T2.20 的真机探针在隔离环境里起真 Claude Code。

## 已拍板（G1，2026-11-09，防翻案）
- A1 一律写键；A2 卡住只识别、报告，不代点同意。
- O1 动态超过 6 小时不再送；超过 15 分钟的带上发生时间。
- O2 `blocked` 时 Moss 页给「打开 Moss 的窗口」按钮，在用户常用的终端里接上 Moss 的 tmux 窗格。
- A3–A5 照上文。
