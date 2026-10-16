# 立项依据与证据（002-menubar）

核实日期：2026-10-16。

## 驱动

| # | 缺口 | 证据 |
|---|------|------|
| D1 | 会话状态只有打开终端敲 `t` 才看得到；哪个会话在等你回话、哪个断了，不看就不知道 | Stage 1 的选择器是唯一入口（`crates/cc-moss/src/pick/`）；`ls --json` 已有 `busy` 字段，`busy=false` 且 running 即「在等你」，但没有任何常驻的地方展示它 |
| D2 | 重启之后，要记得打开终端、运行 `cc-moss restore`，才会发现有会话断了 | 001 设计 R8：不常驻、不自动恢复，恢复由用户执行；开机后没有任何东西提醒用户（`001-cli/DESIGN-cli.md` §5「不会自动启动」） |
| D3 | 想去某个会话，得先找到一个终端窗口，再敲 `t` 选 | `cc-moss attach` 只能在终端里执行（`crates/cc-moss/src/cmd/attach.rs`，`exec tmux attach-session`） |
| D4 | 用户原始需求：终端版之后做 macOS 原生的常驻状态栏 App | 用户原话（2026-10-15）：「有了终端版本之后，我们考虑做 macOS 原生的常驻状态栏的原生 App 版本」 |

## Stage 1 留下的接口（已生效）

- `cc-moss ls --json`：`{"schema":1,"sessions":[…]}`，每项有 `id` `name` `status` `busy` `last_prompt` `cwd` `tmux`
  `notes` `created_at` `last_active_at` `ended_at` `end_reason`；实测本机 0.2 秒（含进程存活对账）
- 单一写入方：只有 CLI 与钩子写 `registry.db`（`001-cli/DESIGN-cli.md` §8）
- 写操作命令齐全：`restore [--yes] [name…]`、`close`、`note`、`new`、`attach`

## 外部事实

| 项 | 结论 | 来源 |
|----|------|------|
| Ghostty 外部开窗带命令 | 1.3 起有 AppleScript 字典：`new surface configuration` 设 `command`，再 `new window with configuration`；本机 1.3.1 实测字典存在 | `Ghostty.app/Contents/Resources/Ghostty.sdef`；https://ghostty.org/docs/features/applescript |
| iTerm2 / Terminal.app | 都有 AppleScript：iTerm2 `create window with default profile command`，Terminal `do script` | 两者的公开脚本字典 |
| Warp、VS Code 内置终端 | 没有从外部开窗并执行命令的稳定接口 | — |
| Homebrew 与未签名 App | Homebrew 4.7 去掉了 `--no-quarantine`，并宣布 2026-09-01 起不再支持无法通过 Gatekeeper 的 cask；Apple Silicon 只运行带签名的原生代码（ad-hoc 签名也算） | https://github.com/ivoronin/TomatoBar/issues/102 |
| 本机签名身份 | 0 个（无 Developer ID 证书） | `security find-identity -v -p codesigning` |
| 工具链 | Swift 6.4（命令行工具），Xcode 已装但未选为活动开发目录；macOS 26.6 | `swift --version`、`xcode-select -p` |

## Claude Code 钩子事实（A13–A16 的依据）

来源：https://code.claude.com/docs/en/hooks.md （2026-10-16 查阅，本机 Claude Code 2.1.301）

- `Notification`：只能做副作用，不能阻塞或改变流程；`notification_type` 有 `permission_prompt`（等待约 6 秒后才发）、
  `idle_prompt`（回答完约 60 秒无输入才发）、`elicitation_dialog`、`agent_needs_input`、`agent_completed` 等
- `PermissionRequest`：可返回 `allow`/`deny`；钩子阻塞期间终端里的提示框行为**文档未说明**——留给 004 实测
- 终端里点了同意/拒绝/关掉提示框，**没有对应事件**；工具跑完会有 `PostToolUse`/`PostToolUseFailure`，被拒则什么都没有
- `Stop` 只表示这一轮回答结束，不覆盖「在等权限」和「在等提问回答」
- `AskUserQuestion` 可由 `PreToolUse` 钩子代答；MCP 的 `Elicitation` 可由钩子代答——都留给 004
- 现状：cc-moss 只注册了 `SessionStart`、`UserPromptSubmit`、`Stop`、`SessionEnd`（`moss-core/src/settings_json.rs:20`）
