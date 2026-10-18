# 代答探针：AskUserQuestion 能不能从外面答（2026-10-18，Claude Code 2.1.302）

## 方法

- 真实 `claude`（`claude --version` → `2.1.302 (Claude Code)`，模型 `--model haiku` 省额度）跑在独立 tmux socket
  `tmux -L cc-moss-probe` 里，`send-keys` 打字、`capture-pane -p` 读界面，没有碰默认 tmux server。
- 工作目录是 scratch 下的临时探针目录 `<probe>/work`，hook 只写在它的项目级 `.claude/settings.json`：
  - `PreToolUse`，matcher `AskUserQuestion`，`timeout: 10`
  - `PermissionRequest`，matcher `*`，`timeout: 10`
  - `PostToolUse`（`*`）、`Notification`：只存档，用来看时间线
- 启动参数加 `--setting-sources project,local`，用户自己的设置（以及里面已装的 cc-moss hook）不参与；
  另设 `CC_MOSS_STATE_DIR` 指向 scratch，防万一写到真实状态目录。
- 同一个 hook 脚本读 `<probe>/mode` 文件决定怎么回应，同一会话里换文件内容就换方式，不必重启：

| 方式 | hook | 输出 |
|------|------|------|
| A 不表态 | PreToolUse | 无输出，exit 0（PermissionRequest 对 AskUserQuestion 也不输出） |
| B | PreToolUse | `permissionDecision: "allow"` + `updatedInput` = 原 `tool_input` 加 `answers` |
| C | PreToolUse | `permissionDecision: "deny"` + `permissionDecisionReason: "用户已回答：<选项>"` |
| D 超时 | PreToolUse | `sleep 25`，超过 10 秒的 `timeout` |
| E（追加） | PermissionRequest | `decision.behavior: "allow"` + `updatedInput` 带 `answers` |
| F（追加） | PermissionRequest | 先 `sleep 7` 再按 E 回答，看对话框是否同时可答 |

每种方式在普通模式（`--permission-mode default`）和 bypass（`--dangerously-skip-permissions`）各跑一次。
提示词：「请直接调用 AskUserQuestion 工具问我：选 A 还是 B（选项 A、B）。拿到回答后只回复"收到：<答案>"」，
每轮换一组选项（甲/乙、红/蓝……），hook 一律选第二个选项，方便区分是谁答的。

下面的输入已脱敏：省略了 `transcript_path`、`scratchpad_dir`，`cwd` 写成 `<probe>/work`，ID 截断。

### hook 收到的输入

PreToolUse（bypass 会话；普通模式只有 `permission_mode` 是 `default`）：

```json
{"session_id":"380b2e3d-…","cwd":"<probe>/work","prompt_id":"2aadf3d1-…","permission_mode":"bypassPermissions",
 "effort":{"level":"medium"},"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion",
 "tool_input":{"questions":[{"question":"选 A 还是 B？","header":"A 还是 B",
   "options":[{"label":"A","description":"选项 A"},{"label":"B","description":"选项 B"}],"multiSelect":false}]},
 "tool_use_id":"toolu_012o…"}
```

PermissionRequest（同一次提问，紧跟在 PreToolUse 之后，同一秒）：

```json
{"session_id":"380b2e3d-…","cwd":"<probe>/work","prompt_id":"2aadf3d1-…","permission_mode":"bypassPermissions",
 "effort":{"level":"medium"},"hook_event_name":"PermissionRequest","tool_name":"AskUserQuestion",
 "tool_input":{"questions":[…同上…]}}
```

要点：`tool_input` 就是 `questions[]`，每题有 `question`、`header`、`options[{label,description}]`、`multiSelect`，
选项完整可拿。**PermissionRequest 没有 `tool_use_id`**，要和 PreToolUse 对上只能靠 `session_id` + `prompt_id` + `tool_input`。

PostToolUse 里答案的形状（B 方式，`tool_input` 与 `tool_response` 都带）：

```json
"answers":{"请选择：甲 还是 乙？":"乙"}
```

即 `answers` 是「问题原文 → 选中的 label」的对象。

## Q1 外部程序能否经 hook 把答案交回去

**行。B（PreToolUse `updatedInput` 带 `answers`）和 E（PermissionRequest allow + `updatedInput` 带 `answers`）都能干净地答；
C 能用但界面显示成报错。**

| 方式 | 普通模式 | bypass | 界面 | Claude 之后 |
|------|----------|--------|------|-------------|
| B | 行 | 行 | 不弹对话框，直接显示 `User answered Claude's questions: · 请选择：甲 还是 乙？ → 乙` | 「收到：乙」 |
| C | 行（凑合） | 行（凑合） | `Error: PreToolUse:AskUserQuestion hook error: 用户已回答：蓝` | 「收到：蓝」，模型把理由当答案用了 |
| E | 行 | 行 | `User answered Claude's questions: · 请选择：山 还是 海？ → 海` 加一行 `Allowed by PermissionRequest hook` | 「收到：海」 |

时间线（普通模式，B）：`17:12:06 PreToolUse` → `17:12:06 PostToolUse`，中间没有 PermissionRequest、没有 Notification。
时间线（普通模式，E）：`17:13:28 PreToolUse` → `17:13:28 PermissionRequest` → `17:13:28 PostToolUse`。

hook 实际输出（B）：

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow",
 "updatedInput":{"questions":[…原样…],"answers":{"请选择：甲 还是 乙？":"乙"}}}}
```

hook 实际输出（E）：

```json
{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow",
 "updatedInput":{"questions":[…原样…],"answers":{"请选择：山 还是 海？":"海"}}}}}
```

关键追加发现（F）：**PermissionRequest hook 运行期间，终端里的对话框已经显示，用户照样能答。**

- F 不动终端：`17:16:53 PreToolUse` / `PermissionRequest`，第 5 秒截屏对话框在（`☐ 春还是秋 … ❯ 1. 春  2. 秋`），
  第 7 秒 hook 回答，对话框收起，显示 `→ 秋` + `Allowed by PermissionRequest hook`。
- F 抢答：hook 还在 sleep 时在终端按 `1`，立刻显示 `→ 茶`，Claude 回「收到：茶」，PostToolUse 的 `answers` 是 `{"选 茶 还是 咖啡？":"茶"}`。
  hook 进程没被杀，7 秒后照常输出了「咖啡」的回答，但被丢弃，没有第二次 PostToolUse。

反观 PreToolUse：hook 运行期间界面只有 `running PreToolUse hook` 转圈，**对话框要等 PreToolUse 结束才出来**（见 Q3 的 D）。

## Q2 bypass 会话里 PermissionRequest 会不会触发

**分情况：对 AskUserQuestion 会触发；对普通工具（Bash）不触发。**

| 场景 | 普通模式 | bypass |
|------|----------|--------|
| `touch probe.txt`（Bash） | 触发，hook 回 allow 后直接执行，界面 `Allowed by PermissionRequest hook` | **不触发**，只有 PostToolUse，直接执行 |
| AskUserQuestion | 触发 | **触发**（`permission_mode: "bypassPermissions"`） |

普通模式 Bash 的 PermissionRequest 输入（节选）：

```json
{"permission_mode":"default","hook_event_name":"PermissionRequest","tool_name":"Bash",
 "tool_input":{"command":"touch probe.txt","description":"Create empty probe.txt in current directory"},
 "permission_suggestions":[{"type":"addDirectories","directories":["<probe>/work"],"destination":"session"},
                           {"type":"setMode","mode":"acceptEdits","destination":"session"}]}
```

bypass 下 Bash 时间线：`17:16:24 PostToolUse`，前面没有 PermissionRequest，与 facts §2 的推断一致；
AskUserQuestion 是例外——它本身就要人答，bypass 不跳过它，所以 PermissionRequest 照发。
因此 **PermissionRequest 方案（E/F）在两种模式下都可用**。

bypass 风险确认框：用 `--setting-sources project,local`（不加载用户设置）启动 `--dangerously-skip-permissions`，
**没有弹确认框**，直接进入 `⏵⏵ bypass permissions on`。用户设置里确有 `"skipDangerousModePermissionPrompt": true`（只读查看），
但本次排除了用户设置也没弹，说明在 2.1.302 上这个开关要么不受 `--setting-sources` 约束，要么已有别处记住了接受；
本探针分不出是哪种，留给 T1.2 在临时 HOME 下核实。`~/.claude.json` 里没查到 bypass / danger 相关字段。

## Q3 hook 不表态或超时后，会话是否照常提示、用户还能在终端答

**行。两种都会回落到 Claude Code 自己的对话框，用户在终端照常作答，Claude 照常继续。**

不表态（A）：
- 普通模式：`17:11:23 PreToolUse` → `17:11:23 PermissionRequest`（也不表态）→ 对话框立刻出现 →
  `17:11:29 Notification permission_prompt`（约 6 秒后）→ 终端按 `2` → `User answered … → B` →「收到：B」。
- bypass：同样，`17:14:32 PreToolUse` / `PermissionRequest` → 对话框 → `17:14:38 Notification` → 终端按 `1` →「收到：A」。

对话框截取（两种模式一样）：

```
 ☐ 选择
请选择：A 还是 B？
❯ 1. A
     选项 A
  2. B
     选项 B
  3. Type something.
  4. Chat about this
Enter to select · ↑/↓ to navigate · Esc to cancel
```

超时（D，PreToolUse `timeout: 10`、hook `sleep 25`）：
- 普通模式：`17:12:55 PreToolUse` 开始，前 10 秒界面只有 `✻ Warping… (running PreToolUse hook · 5s …)`，**没有对话框**；
  `17:13:05` 超时 → PermissionRequest → 对话框出现 → `17:13:11 Notification` → 终端按 `1` →「收到：猫」。
- bypass：`17:15:36` → `17:15:46` 超时 → 同上，终端按 `2` →「收到：狗」。
- 超时本身界面上没有任何报错提示，Claude 也不知道发生过超时。

代价：PreToolUse hook 等多久，终端用户就多等多久才看到题目。

## 给 T1.3 的推荐

**推荐用 PermissionRequest hook（E/F 方式）代答 AskUserQuestion**，不用 PreToolUse：

1. 回填方式：`hookSpecificOutput.decision = {behavior: "allow", updatedInput: {questions: <原样>, answers: {<question 原文>: <label>}}}`。
   多选题的值格式本探针没测（只测了单选），T1.3 先按 label 用 `", "` 连接实现，并补一次多选实测。
2. 理由：
   - PermissionRequest 运行时终端对话框**同时可答**，谁先答算谁的；PreToolUse 运行时对话框被挡住。
   - 普通模式、bypass 都会触发（Q2）。
   - 界面是正常的「User answered」，不像 C 那样显示成 hook error、也不靠模型去理解拒绝理由。
3. 默认等待时长：hook 的 `timeout` 可以放长，**建议 300 秒**（与 `askUserQuestionTimeout` 的 5 分钟档对齐），因为等待期间
   不挡终端；超时后对话框本就还在，等于自然回落。上限不要超过 600 秒（同步 hook 硬上限）。
4. 要处理的边界：
   - 用户在终端先答了，hook 还在等——hook 自己不会被杀，晚到的回答被丢弃。T1.3 要靠该会话的 PostToolUse（AskUserQuestion）
     或下一次 UserPromptSubmit/Stop 把待答题撤掉，并让 hook 尽快退出。
   - PermissionRequest 没有 `tool_use_id`，用 `session_id` + `prompt_id` + `questions` 识别同一道题。
   - 外部没给答案时 hook **不输出**、exit 0，等同 A，会话照常问用户。
5. PreToolUse 只用来**观察**：它带 `tool_use_id` 和完整 `questions`，可以更早拿到题目，但不在里面等，必须立刻 exit 0。
6. 退路：B（PreToolUse `updatedInput`）也已实测可用，可作备选；若将来 PermissionRequest 对 AskUserQuestion 不再触发，
   改用「PreToolUse 短等（≤10 秒）+ B」。C 只作最后退路（会显示成 hook error）。若 B、C、E 都失效，按 K1 退回只提醒、
   不代答，用户回终端答。

## 不留痕

- `shasum ~/.claude/settings.json`：开工前 `0b078f0d2c8a8e7815fca98cbb60caf2007dff66`，结束后 `0b078f0d2c8a8e7815fca98cbb60caf2007dff66`，两次相同。
- 探针结束 `tmux -L cc-moss-probe kill-server`，`tmux -L cc-moss-probe ls` → `no server running on …/cc-moss-probe`。
- 探针 scratch 目录（hook 脚本、日志、临时状态目录）已删除。

## 未覆盖

- 多选题（`multiSelect: true`）、一次多题、用户选「Type something」自填时 `answers` 的形状
- 用户在终端按 Esc 取消时 PermissionRequest hook 的去向
- 其他 Claude Code 版本（只测 2.1.302）
- bypass 风险确认框在全新 HOME 下的行为（交给 T1.2）

## 补测（T1.3，2026-10-18，Claude Code 2.1.302）：多选、自由文字、多题

方法同上：真实 `claude --setting-sources project,local --model haiku --dangerously-skip-permissions`，独立 tmux socket
`tmux -L cc-moss-probe2`，hook 只写在 scratch 探针目录的项目级 `.claude/settings.json`。这次 hook 直接跑 T1.3 的
`cc-moss hook pre-tool-use / permission-request / post-tool-use`（外面包一层只记日志的脚本），`CC_MOSS_STATE_DIR` 指向 scratch。

### 终端里答（没有接手方，cc-moss 不等）：PostToolUse 里的 `answers`

| 题型 | 终端操作 | `answers` |
|------|----------|-----------|
| 多选（`multiSelect: true`，苹果 / 香蕉 / 樱桃） | 按 `1`、`3` 勾选，→ 到 Submit，回车 | `{"喜欢哪些水果？":"苹果, 樱桃"}` |
| 自由文字（选「Type something.」） | 按 `3`，直接打字「都不要，白开水」，回车 | `{"喝茶还是咖啡？":"都不要，白开水"}` |
| 一次多题（单选 + 多选） | 第一题按 `2`，第二题勾 `1`、`2`，Submit | `{"去哪座城市？":"上海","带哪些东西？":"伞, 相机"}` |

结论：**多选的值是所选 label 用 `", "`（逗号加空格）连接**（本次勾选顺序与选项顺序相同，分不出按哪个排）；**自由文字的值就是用户打的原文**，不加前缀；
**多题时 `answers` 每题一个键，键是问题原文**。与 T1.3 的回填格式一致。多题的界面是顶部一排页签（`☐ 城市  ☐ 携带  ✔ Submit`），
最后有一页「Review your answers」确认。

### 外面答（有接手方，`cc-moss ask answer`）

| 题型 | 命令 | 界面 | Claude 之后 |
|------|------|------|-------------|
| 多题（单选 + 多选） | `ask answer <id> --choice 看展 --choice 家人 --choice 同事` | `· 周末做什么？ → 看展` / `· 和谁一起？ → 家人, 同事` + `Allowed by PermissionRequest hook` | 「收到：…看展；…家人, 同事」 |
| 自由文字 | `ask answer <id> --text '都行，你定'` | `· 晚饭吃面还是饭？ → 都行，你定` + `Allowed by PermissionRequest hook` | 「收到：都行，你定」 |

- hook 输出的 `answers` 与随后 PostToolUse 里的 `answers` 逐字相同（`{"周末做什么？":"看展","和谁一起？":"家人, 同事"}`）。
- PreToolUse 先写的题被 PermissionRequest 沿用：状态目录里只有一份 `<id>.json`，答完是 `{"reason":"answered"}`。
- 等待期间终端对话框照常显示，可以随时答。

### 终端先答、Esc 取消（有接手方）

- 终端先答：hook 在等时按 `2` 选「狗」，Claude 回「收到：狗」；PostToolUse 到达后题目撤回为 `{"reason":"answered-in-session"}`，
  等着的 hook 在 2 秒内（下一次轮询）退出，没有输出。
- Esc 取消：界面显示 `User declined to answer questions`，**没有 PostToolUse**；**Claude Code 直接结束了正在等的 PermissionRequest hook 进程**
  （之后 `pgrep` 查不到），题目文件留着、没写撤回。随后这一轮结束会触发 Stop，cc-moss 装好的 Stop hook 会把它撤回
  （`answered-in-session`）；万一没有，题目到 `expires_at` 后也不再列出。

### 普通模式下的 Bash 权限请求（`--permission-mode default`，有接手方）

另起一个探针（`tmux -L cc-moss-probe3`，同样的隔离做法），看 `--safe` 会话的权限请求：

- `touch probe.txt`：PermissionRequest hook 在等时，终端里的「Do you want to proceed?」对话框**照常显示、可以答**；
  在终端按 `1` 同意 → 命令执行 → PostToolUse(Bash) 到达，题目撤回为 `answered-in-session`，等着的 hook 同一秒退出。
- `touch second.txt`：`cc-moss ask answer <id> --allow` → 命令执行，界面 `Allowed by PermissionRequest hook`。
- `touch third.txt`：`cc-moss ask answer <id> --deny --message "先别建这个文件"` → 界面 `Error: 先别建这个文件` +
  `Denied by PermissionRequest hook`，文件没建，Claude 把这句话当成拦截理由转述给用户。

### 不留痕（补测）

- `shasum ~/.claude/settings.json`：开工前后都是 `0b078f0d2c8a8e7815fca98cbb60caf2007dff66`。
- `tmux -L cc-moss-probe2` / `cc-moss-probe3` 各自 `kill-server` 后 `ls` → `no server running on …`；两个探针 scratch 目录都已删除。
