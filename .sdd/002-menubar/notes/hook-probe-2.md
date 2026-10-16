# Claude Code 钩子实测之二：Notification 与 PostToolUse（2026-10-16，Claude Code 2.1.301）

方法：与 `001-cli/notes/hook-probe.md` 相同。真实 `claude` 跑在独立 tmux socket 里，工作目录是临时目录；
用 `--settings <临时文件>` 注入 `Notification`（不设 matcher）、`PostToolUse`（matcher `*`）、`UserPromptSubmit`、`Stop`
四个钩子，每个钩子只把 stdin 追加进日志。加了 `--setting-sources project,local`，用户自己的设置（以及里面已装的
cc-moss 钩子）不参与；`--permission-mode default`，确保会弹权限提示。探针结束后 kill 掉这个 tmux server。

下面的输入已脱敏：家目录下的路径写成 `~`，临时目录写成 `<tmp>`，省略了 `transcript_path`、`scratchpad_dir`。

## 时间线

| 时间 | 事件 | 说明 |
|------|------|------|
| 00:00 | UserPromptSubmit | 「用 Bash 运行 `touch probe-file.txt`」 |
| 00:09 | **Notification** `permission_prompt` | 权限提示出现后约 6–9 秒才发 |
| 00:24 | PostToolUse `Bash` | 在终端里点了 Yes，工具跑完 |
| 00:26 | Stop | |
| 00:37 | UserPromptSubmit | 「用 AskUserQuestion 问我一个选择题」 |
| 00:45 | **Notification** `permission_prompt` | ⚠️ 提问也是 `permission_prompt`，不是 `elicitation_dialog` |
| 01:07 | PostToolUse `AskUserQuestion` | 选了答案之后 |
| 01:08 | Stop | |

## 真实输入

权限提示（Bash）：

```json
{"session_id":"2cd93b1b-…","transcript_path":"~/.claude/projects/…/2cd93b1b-….jsonl","cwd":"<tmp>/work",
 "prompt_id":"fb0b8c86-…","hook_event_name":"Notification",
 "message":"Claude needs your permission","notification_type":"permission_prompt"}
```

提问（AskUserQuestion）：

```json
{"session_id":"2cd93b1b-…","cwd":"<tmp>/work","prompt_id":"6ca5df5e-…","hook_event_name":"Notification",
 "message":"Claude needs your permission","notification_type":"permission_prompt"}
```

PostToolUse（节选）：

```json
{"session_id":"2cd93b1b-…","cwd":"<tmp>/work","permission_mode":"default","hook_event_name":"PostToolUse",
 "tool_name":"Bash","tool_input":{"command":"touch probe-file.txt","description":"…"},
 "tool_response":{"stdout":"","stderr":"","interrupted":false,…},"tool_use_id":"toolu_…","duration_ms":1152}
```

## 结论

| # | 事实 | 对实现的影响 |
|---|------|--------------|
| N1 | Notification 输入有 `session_id`、`cwd`、`message`、`notification_type`，**没有** `session_title`，也没有工具名 | 解析只取 `notification_type` 与 `message`；会话按 `session_id` 找 |
| N2 | 权限提示后约 6–9 秒才发 Notification；用户在这之前就点了同意的话，不会有这条（文档同） | `needs_you` 只代表「等了一会儿还没人理」，不是每次权限请求都有 |
| N3 | `AskUserQuestion` 的提问发的也是 `permission_prompt`，`message` 完全相同；Notification 里分不出是权限还是提问 | 本轮按 spec 映射：`permission_prompt` → `permission`。提问在 2.1.301 上显示为 `permission`；`question` 只来自 `elicitation_dialog`/`elicitation_url_dialog`（MCP 提问）。要区分 AskUserQuestion 需要 PreToolUse，留给 004 |
| N4 | 点了同意后，工具跑完立刻有 PostToolUse（AskUserQuestion 也有，答案在 `tool_input.answers` 里） | 「PostToolUse 清 attention」成立，两种情况都能清掉 |
| N5 | PostToolUse 输入里有完整的 `tool_input`/`tool_response`，可能很大 | 钩子只取 `session_id`，不存其余内容 |

## 未覆盖

- 拒绝（No）或 Esc：按文档没有 PostToolUse；下一次 `UserPromptSubmit`/`Stop` 会清掉 attention
- `elicitation_dialog`、`agent_needs_input` 的真实输入（需要 MCP 服务或后台 agent），按文档的字段处理
