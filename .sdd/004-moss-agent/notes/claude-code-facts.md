# Claude Code 外部事实（S1）

> 2026-10-18 据官方文档整理（WebFetch 摘要，非逐字原文）。标 UNVERIFIED 的是文档没明说或未核实原文，**设计依赖它之前先探针实测**。

## 1. Bypass 权限
- `--dangerously-skip-permissions` = `--permission-mode bypassPermissions`；`--allow-dangerously-skip-permissions` 只把它加进 Shift+Tab 循环。[cli-reference](https://code.claude.com/docs/en/cli-reference)
- `permissions.defaultMode`：`default`(`manual`) / `acceptEdits` / `plan` / `auto` / `dontAsk` / `bypassPermissions`；CLI 参数覆盖设置。[permission-modes](https://code.claude.com/docs/en/permission-modes)
- 项目级 `.claude/settings(.local).json` 里写 `bypassPermissions` **不生效**，回落 Manual；要放 `~/.claude/settings.json`、`--settings` 或 managed。[permission-modes](https://code.claude.com/docs/en/permission-modes)
- 不能从非 bypass 会话中途切进 bypass，只能启动时开。
- **首次以 bypass 启动交互会话会弹警告框**，接受后写 `skipDangerousModePermissionPrompt: true` 到 `~/.claude/settings.json`；可在 user / local / managed 设置预置（项目级无效）。未接受时 tmux 里的会话会停在这个框上。[settings-reference](https://code.claude.com/docs/en/settings-reference)
- 管理员可用 `permissions.disableBypassPermissionsMode: "disable"` 禁用（放 managed；文档两处说法不一，UNVERIFIED）。
- root / sudo 下拒绝 bypass。
- bypass 下 deny 规则照常阻断、ask 规则照常询问；`rm`/`rmdir` 关键路径仍会问。

## 2. 能「替用户回答」的 hook
- **PermissionRequest**：将弹权限询问时触发。输入 `tool_name`、`tool_input`、可选 `permission_suggestions`。输出 `hookSpecificOutput.decision.behavior: allow|deny`，allow 可带 `updatedInput` / `updatedPermissions`，deny 可带 `message`、`interrupt`。allow 不能覆盖 deny 规则。**bypass 下是否触发：UNVERIFIED**（推断不触发——bypass 本就不弹权限框）。[hooks](https://code.claude.com/docs/en/hooks)
- **PreToolUse（AskUserQuestion）**：可返回 `permissionDecision` 与 `updatedInput`；能否用 `updatedInput` 带 `answers` 直接答题：**UNVERIFIED，需探针**。AskUserQuestion 输入（questions / header / options{label,description} / multiSelect）也待核实。`askUserQuestionTimeout` 可设 60s/5m/10m 自动继续；**权限询问从不因空闲自动解决**。[tools-reference](https://code.claude.com/docs/en/tools-reference)
- **Notification**：`notification_type` 有 `permission_prompt`、`idle_prompt`、`elicitation_dialog`、`agent_needs_input` 等；**只能观察，不能应答**。终端里 `permission_prompt` 约 6 秒无输入后触发，`idle_prompt` 约 60 秒。
- **Elicitation（MCP）**：hook 可直接应答（`action: accept|decline|cancel` + `content`）。
- **Stop**：`decision: "block"` + `reason` 让 Claude 继续；连续上限 8 次。**UserPromptSubmit**：可 block、可注入 `additionalContext`，不能改写 prompt，默认超时 30 秒。**SessionStart**：`source` = startup/resume/clear/compact/fork，可注入 `additionalContext` / `initialUserMessage`。注入上限 10,000 字符。
- **超时**：command/http hook 默认 600 秒；同步 hook 最长等 600 秒，超时被取消（超时后是否回落到对话框：UNVERIFIED）。`async: true` 的 hook 不阻塞但决定无效。**等人在别处作答不宜让 hook 同步长等**。

## 3. 从外部驱动 Claude Code
- `-p` + `--output-format stream-json --verbose` + `--input-format stream-json`（仅 print 模式）；stdin 消息 schema UNVERIFIED。
- 续接：`--continue`、`--resume <id|名字|jsonl>`、`--session-id <uuid>`、`--fork-session`；`-p` 恢复已结束会话报错退出 1。
- 人设 / 配置：`--append-system-prompt(-file)`、`--settings <file|json>`、`--mcp-config`、`--plugin-dir`；`--add-dir` 不加载那个目录的 `.claude/`。系统提示在会话首次请求时记录，resume 换参数不生效（除非 `--system-prompt-snapshot off`）。
- **Channels（研究预览）**：MCP server 用 `notifications/claude/channel` 往**运行中的交互会话**推事件；`claude/channel/permission` 可做权限中继（`permission_request` ↔ `permission` allow/deny）。前提：claude.ai 登录或 API key；Team/Enterprise 需管理员开；自定义 server 要 `--dangerously-load-development-channels`，**会弹全屏确认框**；默认白名单只有官方渠道插件；事件无投递确认。[channels-reference](https://code.claude.com/docs/en/channels-reference)
- **Remote Control**：需 Pro/Max/Team/Enterprise 订阅，不支持 API key；App 里不能选 bypass。
- 后台会话：`claude --bg`、`claude attach`、`claude logs`、`claude daemon`；后台会话的权限模式在 supervisor 重启后保持。

## 4. 人设与预置提示的打包
- Output style（Markdown，`keep-coding-instructions`）、skill（`skills/<name>/SKILL.md`，`disable-model-invocation: true` 只在 `/name` 时加载）、旧式 `commands/*.md`。
- 插件：`.claude-plugin/plugin.json` + `skills/` `commands/` `agents/` `hooks/hooks.json` `output-styles/` `bin/` `.mcp.json`；会话级 `--plugin-dir` 加载。`--bare` 跳过 CLAUDE.md、skill、hook、插件、MCP。

## 5. 常驻会话
- 自动压缩在上下文上限处（200K 模型在 200K，1M 模型约 967K）；`autoCompactWindow` 可调；`idleCompaction` 可阻止空闲压缩（默认值 UNVERIFIED）。压缩后 system prompt、CLAUDE.md、memory、MCP 工具重载，已调用 skill 正文重注入（每个 ≤5,000 token），skill 列表不重载。PreCompact / PostCompact hook 存在。

## 对设计的影响
1. R1 的 bypass 要在 user 级设置或启动参数里给；还要让用户**一次性**确认警告框，或预置 `skipDangerousModePermissionPrompt`——否则 tmux 里的新会话会卡在警告框上。这是安全决策，必须由用户明示开启。
2. bypass 下没有权限询问，R3 要代答的就只剩 **AskUserQuestion（选择题）** 和 **idle（等你输入）**；非 bypass 会话还有权限询问可走 PermissionRequest。
3. AskUserQuestion 能否被 hook 直接答掉是 R3 的关键未知 → P0 探针。
4. hook 同步最多等 600 秒；人可能更晚才答 → 代答要有「超时后回落到会话里自己答」的兜底，或走 `cc-moss send` 往 tmux 里敲答案。
5. 往常驻交互会话推外部消息：Channels 需要开发标志并弹确认框，不适合无人值守；可行的是 tmux `send-keys`（cc-moss 已有 `send`）或 `-p --resume` 循环。
6. Moss 人设与 Doctor 预置提示用 output style / skill / `--append-system-prompt-file` 打包，随 cc-moss 安装。
7. 常驻会话要处理自动压缩，状态外置到文件（不能只活在上下文里）。
8. 企业管理策略（`disableBypassPermissionsMode` 等）可能整体禁用 bypass，R1 要能降级并说明。
