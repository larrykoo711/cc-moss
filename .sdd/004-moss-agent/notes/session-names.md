# 会话名与 tmux 名（2026-10-19 调研）

> 起因：用户「用户可能在新建会话时用中文名称，而且英文名称，或者带有空格等等，是无法对应到 tmux name 的，这样几个 name 就无法保持一致了。」

## 现状（核实于 2026-10-19）

- 名字的唯一来源是 Claude Code 的会话名（`--name`、`/rename`），registry 存原样；tmux 名由 `naming::session_name(prefix, name)` 派生（`crates/moss-core/src/naming.rs:11-38`）。
- `sanitize` 只留 `[a-z0-9_-]`，其余一律变 `-`，再去首尾 `-`，空则 `session`：
  - `接口评审` → `session`；两个中文名的会话 → `session`、`session-2`
  - `接口评审 v2` → `v2`；`API Review` → `api-review`
  - 结果：中文名的会话在 `tmux ls` 和状态栏里认不出是哪个，名字「对不上」。
- cc-moss 自己找 tmux 会话一律按会话编号（`$N`）与窗格编号（`%N`）：`attach.rs:32`、`send.rs`、`moss_talk.rs`、`moss.rs`；App 的连接命令是 `cc-moss attach '<名字>'`（`LaunchPlan.swift:39`）。tmux 名只给人看（`tmux ls`、状态栏、手敲 `tmux a -t`）。
- 体检事实 `tmux_name_matches` 拿 tmux 名和「按规则派生的名字」比（`cmd/checkup.rs`）。

## tmux 实测（tmux 3.8，独立 socket）

| 名字 | 能建 | `-t =名字` 找得到 |
|---|---|---|
| `接口评审` `接口 评审` `修复#12` `emoji🚀` `UPPER` `-dash` | ✅ | ✅ |
| `api review` | ✅ | ✅（shell 里需引号） |
| `v1.2` `a:b` `a.b:c` | ✅ | ❌ `.` `:` 是目标分隔符 |
| `$HOME` | ✅ | ❌ `$` 开头被当会话编号（`@` `%` 同理） |

## 方案

1. **规则放宽，只挡 tmux 真正不认的**：保留各种文字（中文、日文、字母数字、emoji）和大小写；空白（含空格、Tab）→ `-`；`.` `:` → `-`；控制字符去掉；shell 里需要转义的 `'"$\`!#` 等 → `-`；去掉开头的 `-$@%=`；连续 `-` 合并；空则 `session`；超过 40 个字符截断（按字符，不切坏中文）。
   - `接口评审` → `koo-mbp_接口评审`；`接口 评审` → `koo-mbp_接口-评审`；`API Review v1.2` → `koo-mbp_API-Review-v1-2`
2. **身份只认编号**：保持现状（cc-moss 从不按 tmux 名找会话），新增一条测试守住「所有 tmux 调用用 `$N`/`%N`」。
3. **重名**：仍加 `-2`、`-3`；「对不对得上」按「派生名或派生名-数字」判断。
4. **已有会话**：下次有事件时按新规则改 tmux 名（按编号改，已有逻辑）；不迁移 registry。
5. **对不上的修法**（体检）：由守护进程代算 `tmux_name_matches` 写进 Moss 的家（F4），不对就按编号改回派生名。

## 不做

- 拼音转写（`接口评审` → `jie-kou-ping-shen`）：要带字典、读着也不如原文；tmux 本来就支持中文。
