# 运行中切换 API 供应商：实测（2026-10-20）

- 版本：Claude Code 2.1.303；cc-switch v4.0.16（源码 `src-tauri/src/live/project/claude.rs` + README）
- 方法：两个本机假 Anthropic API（127.0.0.1，stdlib Python，记录端口 / 鉴权头前缀 / model），被测 claude 跑在隔离 tmux socket，`env -i` + 临时 `CLAUDE_CONFIG_DIR` + `--setting-sources user`，只用假 key
- 未验证：真实 OAuth 订阅 ↔ 第三方的热切（不碰真实凭据）；cc-switch Routing 代理模式实跑

## cc-switch 怎么写

直连模式原子写 `~/.claude/settings.json`：`env` 的 `ANTHROPIC_BASE_URL`、`ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_API_KEY`、`ANTHROPIC_MODEL`、`ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS,FABLE}_MODEL`，顶层 `model`/`apiKeyHelper`。切回官方订阅时删掉这些键，README 写「restart the tool」。Routing 模式：base URL 固定 `127.0.0.1:15721`、占位 token、别名模型，切换只改代理上游。

## 运行中会话对改动的反应

| 改动 | 下一条消息生效 | 证据 |
|---|---|---|
| 改 `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` 的值 | 是（文件监听，≈0.5 s） | 请求从端口 A/key A 变成 B/key B；5 次来回全对；写完几毫秒内就发的那一条仍走旧值 |
| 新增 `ANTHROPIC_AUTH_TOKEN` | 是，优先于 apiKeyHelper | 请求鉴权变成新 token |
| 改 `ANTHROPIC_DEFAULT_*_MODEL` | 是 | 请求 model 变成新别名 |
| 改 `ANTHROPIC_MODEL` / 顶层 `model` | 否 | 请求仍用启动时的模型（官方文档：`model` 只在启动读） |
| 删除 env 键 | 否，旧值留在进程里 | 删 URL 与 token 后请求仍打旧端口 |
| shell env 与 settings `env` 冲突 | settings 优先 | |
| apiKeyHelper 换 key | 是，TTL 过期后第一条仍旧 key，第二条新 key | |

`/status` 显示当前生效的 base URL、token 来源、model，可用来确认。`/model` 会把 `model` 写回 user settings。

## 原地重开

`tmux respawn-pane -k -t <pane> '<启动命令> --resume <session_id>'`：pane id、window id 不变，历史完整，新消息写进同一个 transcript，新进程读新配置。忙碌中重开：用户消息还在、那一轮回复丢失，发 continue 后正常。不要用 `--continue`（同目录多会话会接错）。
