---
name: moss-doctor
description: Give the user's sessions a checkup (体检) or tidy them up (整理会话): read the facts, fix what needs no asking, ask about the rest with buttons, and send a short brief. Use when the user asks for a checkup or a tidy-up, for the daily checkup cc-moss sends, and when the user taps an option you offered.
---

<!-- Written by cc-moss and replaced on every install. -->

# The checkup

## 1. Read the facts

```sh
cc-moss doctor --sessions --json
```

It gives facts, not verdicts. Per session: `name`, `auto_named` (no real name), `folder`, `folder_exists`,
`status` (running / interrupted = dropped), `dropped_for_secs`, `idle_for_secs` (open and doing nothing),
`working`, `waiting_on_you` (permission / question / input), `tmux_name_matches`. And `install.hooks`,
`install.alias_links`.

`"unknown"` means cc-moss could not tell. Unknown is never broken: leave it alone and don't mention it.

## 2. Sort and act

| What you find | What you do | Ask first? |
|---|---|---|
| `install.hooks` or `install.alias_links` false | `cc-moss moss act setup-fix --wait` | no |
| `tmux_name_matches` false | `cc-moss moss act sync-name <session> --wait` | no |
| `auto_named` true | name it after its folder (`ofoxai`, then `ofoxai-2` if taken): `cc-moss moss act name <session> <name> --wait`; not while `working`: it is skipped, try next time | no |
| dropped and `folder_exists` false | `cc-moss moss act forget-gone <session> --wait` (it can't come back anyway); say so in the brief | no |
| dropped, folder still there | offer `[[buttons: 全部找回 \| 只找回今天的 \| 都不要了]]` (bringing back starts work and spends usage) | yes |
| dropped over 7 days | offer `[[buttons: 不再提示 \| 留着]]` | yes |
| open and idle over 3 days | offer `[[buttons: 关掉 \| 留着]]` (closing ends it) | yes |
| `waiting_on_you` set | don't touch; say how many wait in 待处理 (To Do) | — |

Use the session's id (`session_id`) or its name as `<session>`. Never act on Moss; cc-moss refuses anyway.
Each `moss act` prints one line of JSON: `done`, `skipped` (it changed; say nothing or "next time"),
`refused` or `failed` (say plainly what didn't work).

## 3. The brief

A few lines, not a report. In the user's language. First a one-line verdict, then what you fixed and what
they need to decide, then the buttons for the decision:

```
体检完了，大体都好。
· 修好了：补了 2 个会话的名字，钩子重新装好了
· 要你定：4 个断了的会话，要找回吗？
[[buttons: 全部找回 | 只找回今天的 | 都不要了]]
```

Nothing wrong and nothing done: just "体检完了，都好好的" (or "All good: nothing needed fixing."). Leave out
empty lines ("· 修好了" with nothing after it). One question at a time; ask the most important one first.

When the request says it is the daily checkup and asks for `[[silent]]`, add `[[silent]]` to the brief
unless something needs the user's decision; then speak.

## 4. After the user taps

The tap arrives as their message. `restore`, `forget` and `end` need it: pass its id with `--because`.
Find the id with `cc-moss moss chat --json --limit 5`: the latest message `from: "you"`, whose text is
what they tapped.

```sh
cc-moss moss act restore <session> --because <their message id> --wait
cc-moss moss act forget <session> --because <their message id> --wait
cc-moss moss act end <session> --because <their message id> --wait
```

Do exactly what they chose, for the sessions you asked about, then say in one line what happened. A
refusal ("too old", "does not answer a question") means ask again; never work around it.

## 整理会话 (tidy up)

The same flow, focused on names (`auto_named`, `tmux_name_matches`), old dropped sessions and sessions
idle for days. Skip the install checks unless they are broken.
