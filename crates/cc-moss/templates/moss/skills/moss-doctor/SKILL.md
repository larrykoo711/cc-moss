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

Every question that leads to `restore`, `forget`, `end`, `archive` or `purge` carries one `[[confirm: <action> <session>]]`
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
`archive`, `purge` and `purge-archived` need it: pass its id with `--because`, once per session the button names, for exactly that action.
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
To archive one session (it must not be running), ask first with `[[confirm: archive <session>]]`
(the button reads «归档 <name>»), then `cc-moss moss act archive <session> --because <their message id> --wait`.

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

`[[confirm: purge-archived]]` names no session: it covers the sessions the user or the daily tidy-up
archived, and ones archived over a day ago, never one you archived just now. cc-moss words its yes button
with their names («删除 a、b、c»), or their number and the first two («删除 12 个归档会话（a、b 等）»). After the tap:

```sh
cc-moss moss act purge <session> --because <their message id> --wait
cc-moss moss act purge-archived --because <their message id> --wait
```

A session archived after you asked is not covered: it stays. Say in one line how many were deleted and that
the conversation files are in the Trash. No archived sessions: say so, and ask nothing.

## When the user hands you a failure

The panel sends what didn't work, for example 「找回」会话 api（0837a145，~/code/api）失败了：missing_dir：its
folder is gone. Look the session up first: `SESSIONS.md` and `cc-moss doctor --sessions --json` (by its id
prefix or name). Then say in one or two lines what happened and what can be done, in the user's words:

| Reason | What it means | What to offer |
|---|---|---|
| `empty` | it never got a first message: nothing to resume | `[[confirm: forget <session>]]` |
| `missing_dir` | its folder is gone (moved or deleted) | if `folder_exists` is false, `cc-moss moss act forget-gone <session> --wait`; if the folder was moved, ask them to move it back, then restore again |
| `cwd_forbidden` | its working directory is a protected system, home or configuration location | explain that this history cannot be relaunched there; offer a new session in a safe project folder, never silently relocate it or change its configuration source |
| `cwd_missing` | its working directory no longer exists | ask the user to put the original project folder back; do not guess another directory |
| `cwd_not_directory` | the recorded working location is not a directory | explain the working-directory problem; changing settings folders cannot fix it |
| `cwd_unreadable` | the working directory cannot be accessed | ask the user to check its access permissions; do not describe it as missing |
| `cwd_unavailable` | the working-directory safety check could not finish | report that it could not be confirmed; do not retry automatically or bypass the check |
| `cwd_invalid` | the working directory or launch context is not a safe absolute path | ask for a valid project folder for a new session, without editing old records |
| `cwd_changed` | the working directory changed during the operation | ask the user to start again and confirm the current target; never reuse the stale choice |
| `exited_at_once` | Claude Code quit right after starting | ask them to run `claude` in that folder to see why (login, update, settings); then restore again |
| `tmux_failed` | tmux couldn't start it | ask them to try again; if it keeps failing, `cc-moss doctor` in a terminal |
| `config_source_unknown` | the old record does not say which configuration source was used | ask the user to choose the source in the panel for this restore, or explicitly run `cc-moss restore --config-source default --yes <session>` / `cc-moss restore --config-dir <absolute-existing-directory> --yes <session>` themselves; never choose for them, infer a directory from the settings path, or reuse an earlier restore consent for this choice |
| `config_source_changed` | the recorded source changed while the user was choosing | ask the user to start recovery again in the panel and confirm anew; never reuse the earlier choice |
| `config_source_unsupported` | configuration arguments cannot be safely replayed | ask the user to resume manually with the original arguments; choosing a source does not bypass this check |
| `invalid_config_source` | the chosen source is invalid | ask the user to select an absolute, existing configuration directory in the panel; do not copy or change configuration |
| `resume_preflight_failed` | a safety check failed before launch | explain the reported check; do not treat it as a tmux problem or retry with different settings |
| `already_running` | it is running, or coming back through another copy | nothing to do: point at it with `[[sessions: <session>]]` |
| `ambiguous` | the name fits several sessions (`candidates` are their ids) | name each by id with `[[sessions: <id> \| <id>]]` and ask which; then `[[confirm: restore <id>]]` for that one |
| `busy` (restart, archive) | it is in the middle of a turn | wait until it's done; restart queues it by itself |
| `running` (archive) | it still runs | `[[confirm: end <session>]]`, then archive it |
| `not-in-tmux` (restart) | it runs outside tmux | `cc-moss adopt` in a terminal moves it into tmux |
| `moss` | it is you | say cc-moss looks after you |
| `not-found`, `not_found` | no session by that name any more | say so; nothing to do |

Use only the actions and commands in this skill; never make one up. When none fits, say plainly that you
can't fix it from here and what the user can do (often: run the command in a terminal to see the message).
