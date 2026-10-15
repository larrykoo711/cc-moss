# DESIGN — cc-moss Stage 1（终端版）

状态：草案，待用户批准（G1）。依据：`notes/drivers-evidence.md`、`notes/hook-probe.md`、
`notes/current-state-baseline.md`。

## 0. 已拍板（防翻案）

| # | 决策 | 拍板 |
|---|------|------|
| R1 | 项目名、仓库名、命令名、Rust 包名统一为 `cc-moss` | 用户，2026-10-15 |
| R2 | Rust 实现；tmux、starship、`t` 会话工具一起并入，本仓成为这些资产的正本 | 用户，2026-10-15 |
| R3 | `t` 的选择器用 Rust 原生重写，不再依赖 fzf 和 sesh；tmuxinator 装了就支持，不强依赖 | 用户，2026-10-15 |
| R4 | 平台：macOS + Linux（Ubuntu/Debian）。OpenWrt 不支持 | 用户，2026-10-15 |
| R5 | Stage 1 只支持 Claude Code，但接入层抽象成 trait，登记表有 `agent` 字段 | 用户，2026-10-15 |
| R6 | 档案随仓库公开 | 用户，2026-10-15 |
| R7 | 名字以 Claude 会话名为唯一来源，单向同步到 tmux | 用户认可，2026-10-15 |
| R8 | 不常驻、不自动回答提示；恢复由用户执行，执行前先列出待恢复的会话 | 用户认可，2026-10-15 |
| R9 | 一个 tmux 会话只有一个窗口：`new` 和 `restore` 建出的会话里只跑 claude，claude 退出即会话结束；不再支持 tmuxinator 多窗口模板，选择器不设 Windows 标签页 | 用户，2026-10-15（旧 `t.zsh` 的内置多窗口没用上，反而让切换和退出变麻烦） |

## 1. Capability map

| Module id | 职责 | 依赖 |
|-----------|------|------|
| `registry` | 会话登记表：数据模型、状态机、SQLite 存储、并发安全 | — |
| `agent-claude` | Claude Code 接入：解析钩子输入、判定 SessionEnd 的 reason、拼恢复命令、检查会话记录是否存在；实现 `Agent` trait | `registry` |
| `tmux` | tmux 适配：建、改名、列出、attach 会话，按窗格查会话，支持指定 socket；所有调用经一个可注入的命令执行层 | — |
| `hook` | `cc-moss hook <event>`：钩子入口，读 stdin 后更新登记表、同步 tmux 名字；任何错误都不影响 Claude 本身 | `registry` `agent-claude` `tmux` |
| `session-cmd` | 用户命令：`new` `ls` `note` `rename` `close` `restore` `attach` | `registry` `agent-claude` `tmux` |
| `setup` | `cc-moss setup`：把钩子写进 `~/.claude/settings.json`，写入 zsh 集成；写之前先 diff，确认后再写并备份 | `hook` |
| `picker` | `cc-moss pick`（集成后 `t` 指向它）：全屏选择器，列出会话、窗口、目录和待恢复项 | `registry` `tmux` |
| `assets` | `cc-moss install` / `doctor`：内嵌的 tmux.conf、starship.toml、zsh 区块；先 diff 后写、受管区块替换、依赖安装、体检 | — |
| `release` | CI、GitHub Releases、Homebrew tap、README | 以上全部 |

依赖只有一个方向，没有环。接口定义写在提供方的 spec 里。

## 2. 架构

Cargo workspace 分两个 crate：

- `crates/moss-core`（库）：`registry`、`agent`（trait + claude 实现）、`tmux`、`naming`。
  不依赖任何终端界面。Stage 2 的状态栏 App 就复用这一层（通过 `cc-moss ls --json` 或直接只读 SQLite，见 §8）。
- `crates/cc-moss`（二进制）：clap 命令行、`hook`、`setup`、`picker`（ratatui + nucleo）、`assets`
  （用 `include_str!` 内嵌）。

外部依赖只有 `tmux` 和 `claude`；`zoxide` 装了就用，没装就跳过 Folders 标签页。不支持 tmuxinator（R9）。

## 3. 数据模型

核心概念是**槽位（slot）**：一个 claude 进程从启动到退出的整个生命周期。`/clear` 和压缩会换会话 id，
但槽位不变（实测 F4）。登记表以槽位为单位，恢复时用槽位里的最新会话 id。

```text
slot
  slot_id          ULID，首次登记时生成，此后不变
  agent            "claude"
  session_id       当前会话 id（/clear、压缩后更新）
  session_ids[]    历史 id，按时间顺序
  name             Claude 会话名（唯一来源）
  notes            { goal, state, note }，都可为空
  cwd
  launch_args[]    白名单内的启动参数（见 §5）
  pid              最近一次运行的 claude pid
  tmux             { socket, session_id($N), session_name, pane_id(%N) } 或空（不在 tmux 里）
  status           running | closed | interrupted
  end_reason       最近一次 SessionEnd 的 reason
  created_at / last_active_at / ended_at
```

存储：`$XDG_STATE_HOME/cc-moss/registry.db`（默认 `~/.local/state/cc-moss/`），SQLite + WAL。
不用 JSON 文件的原因：十几个会话的钩子会并发写，SQLite 自带锁和事务，Stage 2 也能直接只读。

## 4. 钩子生命周期（状态机）

| 事件 | 处理 |
|------|------|
| `SessionStart source=startup` | 按 `session_id` 找槽位（`new` 预先指定了 id）；找不到就新建。记下 cwd、启动参数、tmux 信息（来自 `TMUX` 和 `TMUX_PANE`，F1）、名字（`session_title`，F2） |
| `SessionStart source=clear\|compact` | 按 `CLAUDE_PID` 找到槽位，把新 id 追加到历史并设为当前 id（F4） |
| `SessionStart source=resume` | 按 `session_id` 在历史里找到槽位，更新 pid 和 tmux 信息，状态改为 running（F9）。若该槽位自己的进程仍活着，则新建槽位，不抢 |
| 任意 `SessionStart` 之后 | 同一进程（pid + 启动时间）的其他 running 槽位记为 closed（`switched`，F11） |
| `UserPromptSubmit` / `Stop` | 更新 `last_active_at`；`session_title` 变了就更新名字，并同步改 tmux 会话名（F2、F3）。事件来自另一个进程或槽位不是 running 时，说明 resume 的 SessionStart 丢了，就地修回 running |
| `SessionEnd reason=clear` | 忽略，后面会紧跟 `SessionStart source=clear`（F4） |
| `SessionEnd reason=prompt_input_exit\|logout\|resume` | 状态改为 closed（`resume` = 会话内切走，F11） |
| `SessionEnd` 其他 reason（包括 `other`） | 状态改为 interrupted（F6） |
| 没有 SessionEnd，进程已经不在 | 执行 `ls` 或 `restore` 时检测：pid 不存在，或 pid 被其他进程复用（比较进程启动时间），就判为 interrupted（F7） |

钩子的硬约束：
- 永远 `exit 0`，不往 stdout 输出（避免 Claude 把输出当成上下文），错误写到 `$XDG_STATE_HOME/cc-moss/hook.log`
- 外部命令（`ps`、`tmux`）500ms 超时；进程启动时间在 `LC_ALL=C`、`TZ=UTC0` 下取（F13）
- 进程身份 = pid 且两侧启动时间都已知且相等；任一未知时不关闭、不接管任何槽位（宁可多留一个待人工判断的槽位，也不静默丢掉一个崩溃会话）
- 交互式判断只看每进程重算的 `CLAUDE_CODE_SESSION_ATTENDED`；`ENTRYPOINT` 会被继承，仅在前者缺失时参考
- 单次执行 p95 < 30ms，SQLite 忙等超时 1s，超时就放弃本次写入（下一次事件会修复状态）
- 不在 tmux 里时，SessionStart 只输出一句提示（通过钩子的 `systemMessage`），说明本会话不受终端崩溃保护

## 5. 启动参数与恢复

- 启动参数来源：`ps -o args= -p $CLAUDE_PID`。只保留白名单内的参数：`--dangerously-skip-permissions`、
  `--permission-mode`、`--model`、`--effort`、`--add-dir`、`--agent`；丢弃 `-p`、`--resume`、`-n`、
  `--session-id`、提示词，以及任何看起来像凭据的值
- `cc-moss restore`：
  1. 列出 interrupted 的槽位，并检查会话记录文件是否存在（F8）；没有记录的标为「空会话，无法恢复」
  2. 展示名字、备注、cwd、最后活动时间，用户勾选（默认全选），确认后执行
  3. 每个槽位：`tmux new-session -d -s <tmux名> -c <cwd> 'claude --resume <id> -n <name> <参数>'`
  4. 最后打印 attach 提示；`--yes` 跳过确认，供脚本使用
- 不会自动启动：开机自动恢复（launchd / systemd 用户单元）留到后续轮次，不在本迭代

## 6. 名字同步

- tmux 名 = `<主机前缀>_<sanitize(Claude 名)>`。主机前缀沿用 `t` 的取法（macOS 用 `LocalHostName`，
  其他平台用 `hostname -s`），可在配置里关掉
- sanitize：只保留 `a-z0-9-_`，转小写，`.` 和 `:` 改成 `-`
- 同一 tmux socket 里重名就加 `-2`、`-3`
- 一个 tmux 会话只有一个窗口（R9）。用户手动在同一会话里再开 claude 时，只有最早登记的那个同步会话名，其余不处理
- 手动改的 tmux 名不会反向同步，用户下次发消息时会被改回来；改名只用 Claude 的 `/rename`（不提供 `cc-moss rename`，见 T1.3 spec）

## 7. 用户入口

| 命令 | 作用 |
|------|------|
| `cc-moss new [name] [-C dir] [--detach] [-- claude 参数]` | 新建 tmux 会话，在里面启动 `claude -n <name> --session-id <uuid> <默认参数>`，然后 attach（已在 tmux 里就 switch-client） |
| `cc-moss ls [--all] [--json]` | 列出槽位：名字、状态、备注、cwd、最后活动时间、tmux 名 |
| `cc-moss note <name> [--goal …] [--state …] [text]` | 写备注 |
| `cc-moss close <name>` | 标为 closed，不再列为待恢复 |
| `cc-moss restore [--yes] [name…]` | 见 §5 |
| `cc-moss attach <name>` | attach 或 switch 到该会话所在的 tmux |
| `cc-moss pick` | 全屏选择器：Sessions / Folders / Interrupted 三个标签页 |
| `cc-moss setup` / `install` / `doctor` | 见 capability map |

配置文件 `~/.config/cc-moss/config.toml`：`claude.default_args`（例如 `["--dangerously-skip-permissions"]`）、
`naming.host_prefix`、`tmux.socket`。

zsh 集成（`cc-moss setup` 写入一个受管区块）：
- `t` 指向 `cc-moss pick`
- 示例说明如何把个人启动别名（如 `alias mycc=…`）改成 `cc-moss new`。不会自动改用户自己的别名

## 8. 给 Stage 2 留的接口

- `cc-moss ls --json` 的输出 schema 带版本号（`"schema": 1`），向后兼容
- 状态栏 App 只读打开 `registry.db`，或者调用 CLI；写操作一律走 CLI，保证只有一个写入方

## 9. 测试策略

- 单元测试：状态机（表驱动，事件序列就用 `hook-probe.md` 里的真实序列）、sanitize、参数白名单、settings.json 合并
- 集成测试：独立 tmux socket（`tmux -L cc-moss-test-<pid>`）+ 临时 HOME + 一个假的 `claude` 脚本
  （按脚本发出钩子事件），覆盖 new → rename → clear → kill → restore 全流程
- 真机验收：用真实 claude 复现 `hook-probe.md` 的探针序列（在独立 socket 里）
- `assets`：在临时 HOME 下 install → 再 install 显示没有变化 → 修改区块 → install 后被替换；
  starship init 永远在最后（坑 #8）

## 10. 风险与开放问题

| # | 项 | 处理 |
|---|----|------|
| Q1 | `source=compact` 和上下文用尽后自动换 id 的情况没有实测 | 实施 `agent-claude` 时补测；状态机已按 `CLAUDE_PID` 归槽，换 id 不会丢 |
| Q2 | 真实关机时 SIGTERM 之后很快就是 SIGKILL，钩子可能来不及执行 | 设计上不依赖 SessionEnd：没有 SessionEnd 也能按 pid 存活判断为 interrupted |
| Q3 | 改 `~/.claude/settings.json` 属于高风险操作 | `setup` 只追加带标记的钩子条目，先 diff、确认、备份；`cc-moss setup --uninstall` 能干净移除 |
| Q4 | Claude Code 的钩子输入字段（`session_title` 等）没有稳定性承诺 | 字段缺失时退化：名字从会话记录文件里的 `custom-title` 读取；集成测试锁定当前版本的行为 |
| Q5 | 选择器重写的工作量 | 放在 MVP 之后的 Phase；MVP 先用 `cc-moss ls` 解决问题 |
