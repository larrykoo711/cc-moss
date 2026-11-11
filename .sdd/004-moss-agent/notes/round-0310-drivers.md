# Round 0.3.10 立项依据（2026-11-09，真机）

本机实测，0.3.9（守护进程与 App）+ Claude Code 2.1.319。会话名、目录一律换成占位（`proj-a`、`~/work/…`）。

## F59 Moss 卡在 bypass 警告框，状态却报「空闲」

- 现象：面板 Moss 页两条消息一直「发送中…」，三点在动。
- 证据：
  - `cc-moss moss status --json` → `"running":true,"activity":"idle","queued":2,"last_reply_at":"2026-10-30T…"`（守护进程 11-06 起重启 Moss 后再没回复过）。
  - `tmux -L cc-moss-moss capture-pane -t moss` → 窗格停在「WARNING: Claude Code running in Bypass Permissions mode … ❯ No, exit / Yes, I accept」。
  - Moss 进程参数：`claude --dangerously-skip-permissions --strict-mcp-config --setting-sources project,local --session-id …`。
  - Moss 家 `.claude/settings.json`（项目级）里有 `skipDangerousModePermissionPrompt: true`；`.claude/settings.local.json` 里没有。
  - `notes/claude-code-facts.md:10` 早有记录：这个键「可在 user / local / managed 设置预置（项目级无效）」。`crates/cc-moss/src/cmd/moss.rs:245` 的注释和实现却把它写进项目级。
- 处置：用户同意后在 Moss 窗格选「Yes, I accept」，Moss 随即开始消化队列。根治未做。

## F60 积压时你的消息排在系统动态后面

- 现象：解卡后 Moss 连做 6 次体检、逐条读动态，你的两条消息仍 `queued`。
- 证据：Moss 收件箱 201 个文件，`from: events` 196 条（最早 11-05）、`from: you` 2 条；按时间先后投递。Moss 窗格里能看到大量重复的「[动态] proj-b 做完了」，同一会话的动态在不同天各来一条。
- 处置：用户同意后把 2 小时前的动态移到 `moss/inbox-backlog-20261109/`（167 个，可移回），队列随即清空。

## F61 同一段对话分出来的会话和原会话同名

- 现象：面板里两行都叫 `proj-a`，一行「模型待确认」。
- 证据（`cc-moss ls --all --json`）：
  - 行 1：`session_id 072d…`，tmux `…_proj-a-2`，`background:false`，创建于 11-05。
  - 行 2：`session_id a8b9…`，`tmux:null`，`background:true`，`name:"proj-a"`，创建于 11-09 10:14:45Z；进程参数 `--session-id a8b9… --fork-session --resume …/072d….jsonl`，父进程是 Claude Code 的 `--bg-pty-host`。
- 复发：F57（0.3.7）修过「`/resume` 交给后台进程后一段对话两行」，F41 修过 `--fork-session` 的进程身份。本次是分叉出的新对话继承了原名，属同一族问题。

## F62 没说过话就退出的会话被列成「断了」

- 现象：面板里 `d458…` 一行显示红色找回图标、「这个会话还没说过话，没什么可找回的」；用户不知道它从哪来。找回报 `empty：跳过：一句话都没发过`。
- 证据：`status:"interrupted"`，`name:null`，`last_prompt:null`，`tmux:null`，`created_at == last_active_at == 2026-11-09T10:14:42Z`，pid 已不在；目录是某个工作区的卡片子目录。与 F61 的分叉相差 3 秒。
- 处置：用户同意后 `cc-moss close d458…`，状态变 `closed`。

## F63 体检里的闲置天数「不更新」：数据没错，是迟到的动态骗了 Moss（归入 F60）

- 现象：Moss 连续几次体检说「proj-c 今天做完了好几轮，却显示闲了 7 天」「几次体检的数字一点没变」。
- 核实（2026-11-09）：
  - Moss 家 `facts.json` 30 个会话的 `idle_for_secs` 与登记表 `last_active_at` 逐个一致（差值 < 0.01 天）。
  - Moss 以为「今天做完」的会话，对应动态在 `chat.jsonl.1` 里的真实时间是 11-02、11-03，也就是闲置天数说的那天。
  - 这些动态积压在收件箱里，今天才投递；动态正文「[动态] proj-c 做完了」不带时间，Moss 把它当成刚发生。
- 结论：闲置数据正确，不单独立项；根因并入 F60：积压的动态不带发生时间、过时了也照样投递。

## 顺带

- 终端里 PATH 上的 `cc-moss` 是 Homebrew 0.3.3，守护进程与 App 是 0.3.9（开发版路径）。不算本轮 bug，交给设备自行升级。
