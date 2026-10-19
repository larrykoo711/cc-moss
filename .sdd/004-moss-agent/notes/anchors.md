# 锚点核实（S1）

> 核实日期 2026-10-18，主干 `691fcef`。逐条实读，不抄文档。S3 写 spec 时超过 7 天或其间有相关大合入要重核。

| 锚点 | 实读结果 | 设计依赖 |
|------|----------|----------|
| `crates/moss-core/src/config.rs:24-29` | `ClaudeConfig.default_args: Vec<String>`，Default 派生，默认空 | R1 改默认 |
| `crates/moss-core/src/settings_json.rs:21-26` | 装 6 个事件：SessionStart、UserPromptSubmit、Stop、SessionEnd、Notification、PostToolUse；没有 PreToolUse / PermissionRequest | R3 要新增可返回决定的 hook |
| `crates/cc-moss/src/cmd/hook.rs:1-5` | 约定「never fail, stay fast」，stdout 只在 SessionStart 输出 `systemMessage`，错误吞掉写 `hook.log` | 新的代答 hook 另立契约，不改旧 hook |
| `crates/moss-core/src/agent/claude.rs:76-91` | Notification 的 `permission_prompt` 同时覆盖权限询问与 AskUserQuestion（注释引 `002-menubar/notes/hook-probe-2.md` N3）；只记 reason + message | 需要选项内容 → 只能从 PreToolUse 的 `tool_input` 拿 |
| `apps/macos/Sources/MossKit/AlertRouter.swift:50` | `AlertRouter`：每条提醒只去一处（通知或 Mo） | R7 路由在此扩展 / 迁到守护进程 |
| `apps/macos/Sources/CCMoss/PetBubble.swift:180-185` | 消息框四段式，detail 区注明「004's approve / deny, not here yet」 | R3 桌面答题 UI 的落点 |
| `apps/macos/Sources/CCMoss/LoginItem.swift:4-35` | 登录项用 `SMAppService` 或 `open -g` 拉起 App，无 KeepAlive | R4 守护进程需要 launchd LaunchAgent |
