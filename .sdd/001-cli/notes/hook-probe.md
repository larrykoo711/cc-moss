# Claude Code 钩子实测（2026-10-15，Claude Code 2.1.300）

方法：用 `--settings <file>` 注入 SessionStart / SessionEnd / UserPromptSubmit / Stop 四个钩子，每个钩子把
stdin JSON 和 `CLAUDE*`、`TMUX*` 环境变量追加写进日志。claude 跑在独立 tmux socket（`tmux -L cc-moss-probe`）里，
用 `send-keys` 驱动。工作目录用临时目录，不碰真实项目。

## 结论

| # | 事实 | 证据（事件序列） | 对设计的影响 |
|---|------|------------------|--------------|
| F1 | 钩子进程继承 `TMUX`、`TMUX_PANE`、`CLAUDE_PID`、`CLAUDE_CODE_SESSION_ID`、`CLAUDE_PROJECT_DIR` | 每条事件的 env 都有 `TMUX_PANE=%0`、`CLAUDE_PID=<pid>` | 钩子能直接定位所在的 tmux 窗格，不需要沿进程树反查 |
| F2 | SessionStart 和 UserPromptSubmit 的输入里带 `session_title`，即当前会话名；Stop 和 SessionEnd 不带 | `SessionStart … session_title=probe-a`；`/rename probe-b` 之后，下一次 `UserPromptSubmit … session_title=probe-b` | 名字同步挂在 **UserPromptSubmit** 上，不挂 Stop。改名后用户发出下一条消息时就会同步 |
| F3 | 没有改名专用的钩子；`/rename` 本身不触发任何钩子 | `/rename` 前后之间没有事件 | 同 F2 |
| F4 | `/clear` 会先发 `SessionEnd reason=clear`（旧 id），再发 `SessionStart source=clear`（新 id）；pid、窗格、名字都不变 | `SessionEnd a166… clear` → `SessionStart ffb3… clear probe-b` | 一个「会话槽」（同一个 pid 和窗格）里的 id 会换。登记表要能把新 id 接到同一个槽上，并且 `reason=clear` 不能算作「用户主动关闭」 |
| F5 | 正常 `/exit`：`SessionEnd reason=prompt_input_exit` | probe-b、恢复后的 probe-term | 这才算主动关闭 |
| F6 | tmux server 被杀（SIGHUP）或进程收到 SIGTERM：**会**发 `SessionEnd reason=other` | probe-hup、probe-term | ⚠️ 推翻了原先的假设「崩溃时 SessionEnd 不触发」。关机走的是 SIGTERM，所以重启前的会话大多会留下 `reason=other`。判定规则改为：**只有 `prompt_input_exit`（以及其他明确由用户发起的 reason）才算关闭**；`other` 和「没有 SessionEnd」都算意外中断 |
| F7 | SIGKILL：没有 SessionEnd | probe-kill | 符合预期，算意外中断 |
| F8 | 会话启动后、发第一条消息前就被杀掉的，没有会话记录，`--resume` 报 `No conversation found` | probe-kill 恢复失败 | 恢复前先检查会话记录文件是否存在；不存在的直接标记为「空会话」，不尝试恢复 |
| F9 | `--resume <id>` 沿用原 id，发 `SessionStart source=resume`，名字自动带回 | `SessionStart ce42… resume probe-term` | 恢复后登记表的主键不变，只更新 pid 和窗格 |

## 未覆盖（实施时补测）

- `source=compact`：压缩后 id 会不会变（Quil 文档说会）
- 上下文用完后自动换 id 的情况（ccmux 文档描述的分叉），以及 `--fork-session`
- `SessionEnd` 其他 reason 的取值（如 `logout`）
- 真实关机（launchd 发 SIGTERM，系统随后发 SIGKILL）时，钩子是否来得及执行完
