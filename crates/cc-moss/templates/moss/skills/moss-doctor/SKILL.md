---
name: moss-doctor
description: Give the user's sessions a checkup (体检) or tidy them up (整理会话): read the facts, fix what needs no asking, archive what nobody used for a while, ask about the rest with buttons, and send a short brief. Also deletes archived sessions (清理归档会话) once the user says yes. Use when the user asks for a checkup, a tidy-up or a clean-up of archived sessions, for the daily checkup cc-moss sends, and when the user taps an option you offered.
---

<!-- Written by cc-moss and replaced on every install. -->

# The checkup

Everything here goes through the shell: cc-moss commands, and `cat`/`>>` for your journal. The file tools
are off for you.

## 1. Read the facts

```sh
cc-moss doctor --sessions --json
```

It gives facts, not verdicts. Per session: `name`, `auto_named` (no real name), `folder`, `folder_exists`,
`status` (running / interrupted = dropped), `dropped_for_secs`, `idle_for_secs` (open and doing nothing),
`working`, `waiting_on_you` (permission / question / input), `tmux_name_matches`, `name_fix` (the
standard form of a name that isn't standard: spaces, dots, too long) and `name_suggestion` (a short name
from the folder for a session without one, already unique). And `install.hooks`, `install.alias_links`.

Archived sessions are not in `sessions`: `archived` has their `count`, their names and ids (`sessions`),
`archived_last_day` (how many went into the archive in the last day) and `archive_after_days` (after how many
idle days cc-moss archives a session that isn't running; 0 = never). The archive is a shelf, not a bin: an
archived session is still restorable, `claude --resume` takes it back out, and the panel brings it back with one tap.

`"unknown"` means cc-moss could not tell. Unknown is never broken: leave it alone and don't mention it.

## 2. Sort and act

| What you find | What you do | Ask first? |
|---|---|---|
| always, first | `cc-moss moss act sync-names --wait`: every tmux name back in line with its session's name | no |
| `install.hooks` or `install.alias_links` false | `cc-moss moss act setup-fix --wait` | no |
| `name_fix` set | `cc-moss moss act name <session> <name_fix> --wait` | no |
| `name_suggestion` set | `cc-moss moss act name <session> <name_suggestion> --wait` | no |
| dropped and `folder_exists` false | `cc-moss moss act forget-gone <session> --wait` (it can't come back anyway); say so in the brief | no |
| dropped, folder still there | ask with `[[confirm: restore <session>]]` for each (bringing back starts work and spends usage) | yes |
| dropped over 7 days | ask with `[[confirm: forget <session>]]` for each, and `[[buttons: 留着]]` | yes |
| open and idle over 3 days | ask with `[[confirm: end <session>]]` for each, and `[[buttons: 留着]]` (closing ends it) | yes |
| `waiting_on_you` set | don't touch; say how many wait in 待处理 (To Do) | — |
| `archived_last_day` above 0 | nothing to do (cc-moss archived them, idle over `archive_after_days` days); one line in the brief: "归档了 5 个 30 天没用的会话" ("Archived 5 sessions unused for 30 days") | no |

Use the session's id (`session_id`) or its name as `<session>`. Never act on Moss; cc-moss refuses anyway.
Names go in only while a session is idle: a busy one comes back `skipped` and is done next time. Use the
names exactly as given; don't invent others. In the brief, names are one line: "补了 2 个名字，改正了 1 个"
("Named 2 sessions, corrected 1").
Each `moss act` prints one line of JSON: `done`, `skipped` (it changed; say nothing or "next time"),
`refused` or `failed` (say plainly what didn't work).

## 3. The brief

A few lines, not a report. In the user's language. First a one-line verdict, then what you fixed and what
they need to decide, then the buttons for the decision:

```
体检完了，大体都好。
· 修好了：补了 2 个会话的名字，钩子重新装好了
· 要你定：2 个断了的会话，要找回吗？
[[buttons: 先不用]]
[[confirm: restore 0837a145]]
[[confirm: restore 5f1e22c0]]
```

Every question that leads to `restore`, `forget`, `end` or `purge` carries one `[[confirm: <action> <session>]]`
per session and action (its `session_id` prefix or name). The markers are not shown. cc-moss adds the yes
button itself, one per action, worded with the sessions it covers and their folders («结束 work · ~/code/work»): only a
tap on that button is a yes. Your own `[[buttons:]]` are for other answers ("not now", "keep it"); a tap
on one of them never lets you act. Don't write your own yes button for these.
A session whose name another open session shares gets no yes button; tell the user to rename one first.

Nothing wrong and nothing done: just "体检完了，都好好的" (or "All good: nothing needed fixing."). Leave out
empty lines ("· 修好了" with nothing after it). One question at a time; ask the most important one first.

When the request says it is the daily checkup and asks for `[[silent]]`, add `[[silent]]` to the brief
unless something needs the user's decision; then speak.

## 4. After the user taps

The tap arrives as their message, its text the button's («找回 api、db»). `restore`, `forget`, `end`,
`purge` and `purge-archived` need it: pass its id with `--because`, once per session the button names, for exactly that action.
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

The same flow, focused on names (`sync-names`, then every `name_fix` and `name_suggestion`, no asking),
old dropped sessions and sessions idle for days. Skip the install checks unless they are broken.
First of all, `cc-moss moss act archive-idle --wait`: it archives every session that isn't running and
wasn't used for `archive_after_days` days (no asking: it can be undone). Say how many in the brief.
To archive one session the user names (it must not be running): `cc-moss moss act archive <session> --wait`.

## 清理归档会话 (delete archived sessions)

Deleting is the one thing that can't be undone from the panel: the session leaves cc-moss's records and its
conversation file goes to the Trash (it can still be dug out of there). Only archived sessions can be deleted;
a session in the list has to be archived first.

The app sends "清理归档会话：<name>" ("Clean up archived session: <name>") for one, and "清理全部归档会话"
("Clean up all archived sessions") for all of them. Read the facts (`archived`), then ask once:

```
要删掉归档里的 api 吗？对话记录会移到废纸篓。
[[buttons: 先留着]]
[[confirm: purge api]]
```

```
归档里有 12 个会话，要全部删掉吗？对话记录会移到废纸篓。
[[buttons: 先留着]]
[[confirm: purge-archived]]
```

`[[confirm: purge-archived]]` names no session: it covers the sessions that are archived when you ask, and
cc-moss words its yes button with their number («删除 12 个归档会话»). After the tap:

```sh
cc-moss moss act purge <session> --because <their message id> --wait
cc-moss moss act purge-archived --because <their message id> --wait
```

A session archived after you asked is not covered: it stays. Say in one line how many were deleted and that
the conversation files are in the Trash. No archived sessions: say so, and ask nothing.
