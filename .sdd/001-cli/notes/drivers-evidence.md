# 001 立项依据（drivers + 证据）

记录日期：2026-10-15。主机名、路径、会话 id 已脱敏。

## D1 机器重启后，进行中的 Claude Code 会话无法定位和恢复

**证据**（2026-10-15 一次真实重启后的排查）：

- 开机时间 09:14，重启前 36 小时内有写入的会话记录共 16 份，其中 3 份为空。
- 要判断哪些会话需要恢复，只能逐份读 `~/.claude/projects/*/*.jsonl`，从末尾的消息推断「停在哪一步、
  在等什么」。最终分成三类：需要恢复 3 个、等外部输入 2 个、已经收尾 8 个。
- 这次排查耗时约 10 分钟，依赖人工判读。这不是第一次发生，用户原话：「这种情况不是第一次出现了，
  相当难受」。

## D2 tmux 只能扛住终端崩溃，扛不住重启；而且 tmux 会话和 Claude 会话之间没有对应关系

**证据**：

- 恢复后，7 个 tmux 会话（`<host>_em-*`）里各跑着一个 `claude`。`tmux list-panes` 显示前台命令都是
  `zsh`，因为 claude 跑在一层包装 shell 之下，按进程名看不出窗格里是不是 claude。
- `claude agents --json` 列出 9 个交互式会话，其中 6 个的名字是自动生成的（`<dir>-1c` 这种），
  和 tmux 会话名对不上。例如 tmux 里的 `em-api-gw` 对应 Claude 里的 `<dir>-1c`。
- 8 个会话的工作目录相同。凡是「按目录找最新会话记录」的恢复方法，在这种情况下必然恢复错。

## D3 会话缺少「停在哪 / 在等什么」的备注

**证据**：D1 排查中，每个会话都要读几百行记录才能得出一句「在等供应商的生产 key」。
Claude Code 自带 `-n/--name` 和 `/rename` 起名，但没有备注字段。

## D4 终端标准资产分散，不是一套可安装的工具

**证据**：tmux.conf、starship.toml、`t.zsh`（tmux 会话工具，0.2.4，464 行）和多机同步脚本
（821 行）目前放在一个私有仓的 skill 模板目录里，只能通过该 skill 的 SSH 下发脚本安装，
别人无法单独使用。

## 已有能力（不重复造）

Claude Code 2.1.300 自带以下能力（`claude --help` 实读）：

| 能力 | 入口 |
|---|---|
| 起名 | `claude -n <name>`、`/rename` |
| 按 id 或名字恢复 | `claude --resume [id|搜索词]`；恢复时另起新 id：`--fork-session` |
| 新会话固定 id | `--session-id <uuid>` |
| 列出活会话（JSON） | `claude agents --json`：name、sessionId、cwd、status、pid |
| 后台会话 | `--bg`、`attach`、`logs`、`stop`、`respawn --all` |

缺的是：重启后自动恢复、tmux 名与 Claude 名的绑定、备注。

## 开源方案调研（2026-10-15）

| 方案 | 借鉴点 | 不直接采用的原因 |
|---|---|---|
| ccmux | 首次启动固定 `--session-id`；按标题追踪换了 id 的会话并更新登记；launchd 开机恢复 | 星标很少；带自更新的常驻进程，还会自动回答提示，不适合开着 bypass 权限的会话 |
| agent-deck | 分字段备注（goal/state/decision/note）；名字锁定；独立 tmux socket | 要接管整个工作方式；文档没写重启后能否恢复 |
| tmux-assistant-resurrect | 恢复时连启动参数一起还原 | 自述：同一目录下的两个 Claude 会话无法区分 |
| tmux-claude-resurrect | SessionStart 钩子按窗格记录；保留历史快照 | 已归档；依赖 tmux-resurrect |
| Quil | `/clear`、压缩会换会话 id，钩子要每次覆盖 | 要换掉 tmux |
| claude-squad | — | 强制一会话一 worktree；不能恢复重启前的会话；AGPL |

链接：
- https://github.com/max-listov/ccmux
- https://github.com/asheshgoplani/agent-deck
- https://github.com/timvw/tmux-assistant-resurrect
- https://github.com/cookiecad/tmux-claude-resurrect
- https://quil.cc/blog/resume-claude-code-session-after-reboot/
- https://github.com/smtg-ai/claude-squad

## 命名

项目名 `cc-moss`（用户定，2026-10-15）。MOSS 是《流浪地球》里的 AI，使命是保护人类文明的延续；
对应本项目的使命：机器崩溃、重启之后，会话和工作照样延续下去。README 与 Stage 2 状态栏 App 的叙事
都沿用这个意象。

## 会话中已定的设计方向（S1 正式设计时需复核）

1. 用 SessionStart / SessionEnd 钩子主动上报会话 id，不按目录猜。SessionEnd 没触发的会话就是意外中断的，
   作为恢复候选。
2. 登记表以 Claude 会话 id 为主键；tmux 会话 id、名字、cwd、启动参数、备注是附带字段。
   不在 tmux 里的会话也登记。
3. 名字以 Claude 会话名为唯一来源，单向同步到 tmux（新建时、SessionStart 时、每轮结束的 Stop 钩子），
   tmux 名非法字符做转义，重名加后缀。
4. 入口改造用户自己的启动别名（如 `mycc`），不包装 `claude` 命令本身。
5. 不常驻、不自动回答提示：恢复由用户执行，先列出待恢复会话再确认。
6. 用户已定：用 Rust 实现；tmux、starship、`t` 工具一起并入，形成一套终端工具；Stage 2 做 macOS 状态栏 App。
