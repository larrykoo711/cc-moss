# Changelog

All notable changes to cc-moss are recorded here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, minor versions may change behaviour.

## [Unreleased]

### Fixed

- One menubar app: `cc-moss install` and `update` update the app where it is (also in `/Applications`), and
  with a copy in both Applications folders they keep `~/Applications`' and move the other to the Trash; `doctor`
  warns about two copies. Before, an app in `/Applications` got a second copy in `~/Applications`, and both ran.
- Moving a session into tmux works for a session started with `--fork-session` or one that went through `/clear`:
  its process still names the conversation it started from, which cc-moss took for another session's.
- `/clear` and compaction no longer move a session's folder to wherever Claude Code happened to be working (a
  worktree it had stepped into, say): the folder stays the one it started in, where `claude --resume` finds the
  conversation, so bringing it back or moving it into tmux keeps working after the worktree is gone.

## [0.3.2] - 2026-10-20

### Added

- **Update tips in the app.** When a newer version is out, a light bar on top of the panel says so, with a link to
  that version on the website's changelog and an Update button (the same `cc-moss update`); its × hides it for that
  version only. Mo tells you once per version in a bubble, by its usual rules. After an upgrade, the first panel
  open shows what the new version brings, from the changelog bundled into the app. The app now checks a little after
  launch and every 6 hours as well as on panel open (still at most once in 6 hours), and never with
  `[update] check = false`, which `cc-moss config get update.check` now reads.

### Fixed

- Moss can look up archived sessions and ones closed in the last 30 days: `cc-moss ls` (and `--all`, `--archived`,
  `--json`) works in Moss's sandbox, reading a session list the daemon writes into its home with nothing anyone
  said. Before, Moss was told to run `ls` but its sandbox couldn't open the records, so it only saw today's list.

## [0.3.1] - 2026-10-20

### Added

- **Archive.** Sessions that ended and saw no activity for 30 days are archived each day
  (`sessions.archive_after_days`, `0` turns it off); running sessions never are. `cc-moss archive` / `unarchive`
  do it by hand (`--end` ends an idle running session first; End and Archive in the app), `ls --archived` lists
  them, and resuming one brings it back by itself. The app keeps them in a folded "Archived" group with Bring Back
  and Clean Up. Clean Up goes through Moss, which asks before deleting; records leave cc-moss and the session's own
  conversation files go to the Trash. Moss also asks before archiving a single session, and Clean Up All leaves out
  what Moss archived in the last day.
- **Switching API providers without starting over.** The daemon notices when Claude Code's settings point at a new
  provider. Most switches apply to running sessions from the next message, and the panel says so. When one can't
  (back to your subscription, or `ANTHROPIC_MODEL` in the settings' `env`), the sessions affected are marked, and
  `cc-moss restart` (Restart / Restart All in the app) restarts them in their own tmux pane, same conversation;
  busy ones wait for their turn to end. `/model` never asks for a restart. Each session is judged against the
  settings file it started with, and `cc-moss doctor` shows the provider's host and warns when sessions read another
  settings folder. Moss follows the switch too; its key comes from `cc-moss moss provider-key`, never from a file it
  can read. Token and key values are never shown or recorded.
- **Waiting on you.** One live card on the Moss page lists every session waiting for an answer or a permission, and
  you can answer them there. Moss is no longer told about each one.
- **When something fails, the panel says so.** Bring back, restart, archive, move into tmux and reply all end in a
  plain-words notice with Copy Details, and Ask Moss to Fix It when Moss is running.
- The gear menu follows a Mac app's own menu (About cc-moss, Check for Updates, Settings, User Guide, Report a Problem, Quit). The
  About page shows the slogan, the MIT license and links to the website and GitHub. Jump to bottom on the Moss page is
  a quiet strip above the composer.

### Fixed

- Headless `claude -p` runs (plugins' background summaries, scripts) no longer pile up as interrupted sessions:
  `claude agents` lists them as interactive, and cc-moss now reads the process's exact arguments before adopting one.
- A background task's notice is no longer shown as a session's last message.
- An ended session brought back from Archived shows in the list again; Bring Back All skips sessions you ended on
  purpose.
- A session stopped with Esc no longer stays "working" until restart.
- Return picks the word while a Chinese input method is composing instead of sending.
- Settings no longer freezes when macOS's login item service stalls.
- The app no longer freezes when macOS's file events service (fseventsd) stops answering: the watcher starts off the
  main thread, and the app looks at the state folder every 2 seconds until it does.
- Moss's yes buttons wrap instead of being cut off; Mo's options show the pointing hand.
- `cc-moss doctor` no longer calls a model setting alone "your own API key".
- Superseded copies of a session close quietly, and `--json` names sessions that share a name.
- The app acts on the session you clicked by its conversation id, not its name: archiving, bringing back,
  restarting, replying or opening one of two sessions that share a name no longer fails as ambiguous.

### Security

- Session names are cleaned of control and invisible characters on the way in, and restart refuses arguments
  that hold them.
- Purge moves only a session's own transcripts inside Claude Code's folder; on Linux the Trash follows
  `XDG_DATA_HOME` and never replaces a file.
- Restart types into a pane only when a known shell holds it; Moss answers the folder trust question only at the
  bottom of the screen, once per start.

## [0.3.0] - 2026-10-19

Sessions start in bypass mode, questions can be answered from the panel or Mo, and Moss moves in: a Claude Code
session that keeps your sessions tidy, runs a daily checkup and answers when you ask.

### Added

- **A cc-moss daemon that stays in the background (macOS).** `cc-moss install` sets it up: it starts when you log in,
  comes back if it's killed, and only one ever runs. `cc-moss daemon status` says whether it's up, for how long and
  which version; `daemon stop` and `daemon start` turn it off and on. Settings has a new Moss page showing the same,
  with Start when it's not running, and `cc-moss doctor` checks it.
- **Moss, a Claude Code session cc-moss keeps running (macOS).** The daemon starts it at login and brings it back,
  fresh, if it's killed or crashes (after 30 s, 2 min, 10 min, then hourly), on a tmux server of its own. It lives in
  `moss/` under the state directory with its own persona (`IDENTITY.md`, `SOUL.md`, `USER.md`), journal and memory;
  install refreshes cc-moss's files there and never touches those. It works on files only through shell commands,
  which run in Claude Code's sandbox and can write only there; Claude Code's file tools are off for it, and a guard
  hook checks every tool call and refuses tools it doesn't know, MCP tools included. It has no network, can't read Claude Code's folder
  (`~/.claude`) or cc-moss's records outside its home, and learns about your sessions from the overview
  (`SESSIONS.md`) and checkup facts (`facts.json`) the daemon writes there; it loads only its own project settings
  (`--setting-sources project`), not yours. The picker and `cc-moss close` can't end it, and the panel's session list
  leaves it out; turn it off in Settings › Moss, with `cc-moss moss stop`, or with `[moss] enabled = false`.
  `cc-moss daemon stop` stops it too. `cc-moss moss status [--json]` says how long it has run, how often it
  restarted, and why it isn't running when it isn't; `ls --json` marks it `"role": "moss"`. When it needs you, the
  menu bar icon says so. It uses your Claude Code sign-in and nothing while it waits. Going back to an older cc-moss
  may show an old Moss as an interrupted session to bring back; `cc-moss close` it.
- **Talk to Moss (macOS).** The panel's side rail has a Moss page (⌘3): how Moss is doing at the top (standing by,
  thinking, waiting for you to confirm, restarting, off), your conversation below, and a quick bar at the bottom with
  a keyboard button for writing your own. The daemon
  pastes what you send into Moss's terminal when it's free, checks it arrived, and brings the reply back; when the
  reply can't be read it says so instead of hanging. Moss also hears when a session needs you, drops or finishes
  (batched, not repeated, checked again before it goes; `[moss] digest = false` turns it off), and its answers to
  routine "turn finished" news stay quiet. A reply rings Mo or a notification unless you're on the Moss page.
  `cc-moss moss say` and `cc-moss moss chat --json` do the same from the terminal.
- **More on the Moss page.** The quick bar holds Checkup, Who's waiting on me, What's stuck, What got done today and
  Tidy up, plus Claude Code's `/usage`, `/status`, `/context`, `/cost` and `/stats` as fits your account (drawn as
  cards for `/usage` and `/status`), ranked by use and by what's going on, with the rest under ⋯ and Moss's own picks
  first. Replies render Markdown, group within 10 minutes, fold when long and carry session cards (open, bring back,
  move into tmux, rename, pin, Handle); session names in them are badges you can point at or click. Moss's guess at
  your next message floats above the bar.
- **Checkup (macOS).** Checkup and Tidy up on the Moss page: Moss fixes what doesn't need you (tmux names, hooks and
  aliases, names for sessions without one, dropped sessions whose folder is gone), sends a short brief, and asks with
  yes/no buttons before bringing back, forgetting or ending anything. It acts through the daemon
  (`cc-moss moss act`), which for those needs your tap on the yes button cc-moss itself puts under the question
  (worded with what it agrees to, such as "End work"), in the last 10 minutes, and counts each tap once; it logs every action to `audit.jsonl`, and can be
  switched off (`moss.actions`). It runs by itself the first time you open the panel each day; Settings › Moss has Daily checkup and Daily brief
  (`moss.daily_checkup`, `moss.daily_brief`). `cc-moss doctor --sessions --json` lists the facts it reads.
- **Sessions page: pins, replies and search (macOS).** Pin sessions to a Pinned section (the pin after the name, or
  ⌘P). Click a session waiting on you to reply on the spot, with Claude Code's own suggestion already typed (also in
  ⌘E); after a send the next waiting session opens, and a busy one queues your message. Search ranks one list,
  highlights matches and says why a row matched, reads abbreviations and pinyin initials, finds sessions closed in
  the last 30 days, and offers to ask Moss when nothing matches. `cc-moss suggest` reads the suggestion.
- **Move a session into tmux.** `cc-moss adopt NAME [--dry-run] [--json]`, or Move into tmux on its row: an idle
  session started outside tmux ends in its old terminal and carries on in tmux, named after its folder if it had no
  name.
- **Open the panel from anywhere (macOS).** Opening the app again (Spotlight, `open -a cc-moss`, the Dock) opens the
  panel, and so do `cc-moss://panel` links (`/sessions`, `/queue`, `/moss`). Settings › General has Appearance:
  System, Light or Dark.
- **Answer a session's question from elsewhere.** When a session asks a multiple-choice question, or a `--safe`
  session asks for permission, cc-moss writes it down: `cc-moss ask list` shows it and
  `cc-moss ask answer ID --choice LABEL` (or `--text`, `--allow`, `--deny`; with several questions `--choice N:LABEL` and `--text N:TEXT`, mixed freely) hands the answer back, and the session
  carries on. The terminal dialog stays up meanwhile, and whoever answers first wins. Sessions only wait while someone
  takes questions (`cc-moss ask take`; the menubar app does it while it's open, Slack will later), for at most 5 minutes
  (`[ask] wait_secs`); otherwise, or if anything goes wrong, nothing changes. Two new hooks, `PreToolUse` and
  `PermissionRequest`: run `cc-moss setup` after upgrading. Turn it off with `cc-moss config set ask.enabled false`.
  Any program running as you can answer, permission requests included; previews hide things that look like keys. An
  older `setup --uninstall` can't remove the two new hooks: uninstall with this version before downgrading.
- **Answer from the panel or Mo (macOS).** While the menubar app is open, sessions wait for it. The panel has a side
  rail: Sessions, and To Do with everything waiting on you, oldest first, one item open at a time with the session's
  name, the whole question and its options as written; answer one and the next opens. A line at the bottom of
  Sessions says when something new is waiting.
  Click an option, tick several and click Done, Type something (2–5 lines, ⌘↩ sends, in Mo's bubble too, on every question of a several-question ask), or
  Allow / Don't Allow a `--safe` session's request;
  several questions are answered one by one and sent together. The card says whether it was answered, too late, or
  saved for later, and disappears by itself when the question is answered in the terminal, times out or the session
  ends. Mo's message box lists the options so a click answers there; otherwise a notification opens the panel on the
  question, and both are taken back once it's gone. A question that arrived while the panel was open goes to Mo or a
  notification when you close it unanswered. A session waiting for you to type gets a reply box in To Do. `cc-moss ask answer --json` says how an answer went in one line (`answered`, `too-late`, `not-waiting`). Switch it off
  in Settings › Notifications › Answer questions here (`ask.enabled`).

### Changed

- **The daemon runs at normal priority and with a PATH.** Its LaunchAgent now says `ProcessType` `Standard` (not
  `Background`, which slowed Moss and everything it starts) and pins a `PATH` with your shell's folders plus where
  Homebrew and Claude Code install, so it finds `claude` and `tmux`, and the config file it reads. Run
  `cc-moss install` after upgrading.

- **New sessions start in bypass mode.** Sessions from `cc-moss new` and the app no longer stop to ask for each
  permission. For normal mode once, use `cc-moss new NAME --safe`; to turn it off for good, run
  `cc-moss config set claude.permission_mode default` or switch off "Let it work on its own" in Settings › General.
  Install sets Claude Code's `skipDangerousModePermissionPrompt` only if you haven't set it, so sessions don't stall on
  the bypass warning, and uninstall takes out only that. A policy that disables bypass is respected, and `doctor`
  says so.
- **The panel has a side rail and one app menu (macOS).** Sessions ⌘1, To Do ⌘2, Moss ⌘3; the gear at the foot holds
  Settings, Refresh, the version or Update, and Quit. The footer has New Session and the selected row's Open. The
  panel picks one of three heights as it opens, by how many sessions you have, and refreshes when sessions change
  (`registry.changed`), at most every 1.5 s (5 s with the panel closed).
- **Moss starts without MCP servers**, and its context is capped at 500k tokens
  (`CLAUDE_CODE_AUTO_COMPACT_WINDOW`), so a session that never ends doesn't keep growing.
- **tmux names keep the session's own name.** Chinese, capitals and emoji stay; only white space, `.`, `:` and shell
  characters become `-`, up to 40 characters. Terminal tabs show the session's own name (run `cc-moss install` for
  the new tmux config).
- **A new session starts with a free name.** When a session already has the folder's name, the form suggests the next
  one (`docs-2`). Typing a taken name outlines the field and says what Return will do (open it, or offer to bring a
  stopped one back), with a free name one click away.

### Fixed

- **Clicking a session selects it.** Hovering no longer moves the selection, so passing over other rows on the way to
  a button doesn't change what it acts on.
- The buttons at the end of a row no longer blink every few seconds as the list refreshes.
- A new session's name field has the keyboard as soon as the form opens.
- A rename from the panel shows even while the session is idle.
- A question's bubble stays put between steps, and the review shows each answer as a quote you can change.

## [0.2.2] - 2026-10-18

Each session carries its own actions, one place for permissions, and a crash fix.

### Added

- **A Permissions page in Settings**: Accessibility, notifications and control of your terminal (Automation), each
  with where it stands, what it's for and Allow when it's missing. A dot beside the page means one is off while
  something uses it. Pages that rely on a permission show the same row where it's missing.
- **Told when an update takes Accessibility away.** The panel says "Allow Accessibility again" at the top, with Allow,
  instead of the shortcut going quiet without a word.

### Changed

- **Each session carries its own actions.** Point at a session and Rename, Send a Message and Open sit at the end of
  its row, with ⋯ for the rest (a note, copying, stop reminding). Right-click it, or press ⌘K, for all of them, right
  beside the session. New session, refresh, settings and quit moved to the cc-moss logo at the bottom left.

### Fixed

- **The app no longer quits when you leave Settings › Shortcuts too quickly.** Moving to another page while it was
  still reading your names could close cc-moss.
- **Choosing a folder for a new session.** The folder list opens right under its button and the panel stays open
  (it used to vanish, and the list could appear far away). Each folder reads as its name and where it lives, on one
  line; type a path such as `~/code` to use any folder; ↑↓ and Return work. "Another folder…" opens the system picker
  above the panel, and you come back to the form as you left it.
- Every "Choose…" in Settings and the welcome opens as a sheet on its own window instead of a separate dialog in the
  middle of the screen.

## [0.2.1] - 2026-10-17

Tabs instead of piles of windows, and every shortcut on one settings page.

### Added

- **Sessions open as tabs.** Settings › General › Terminal has a new "Open as" choice, new tab (the default) or new
  window. Ghostty 1.3 or newer, iTerm2 and WezTerm open real tabs; Terminal opens a tab when cc-moss is allowed under
  Accessibility, otherwise a window; kitty and Alacritty keep opening windows.
- **A Shortcuts page in Settings** (it replaces Aliases): the key that shows the panel (moved from General), the short
  names you type in a terminal, and the keys inside the panel. A short name is set like the panel key: click the
  keycap, type, press Return (Esc cancels, an empty name removes it). Saving plays a little preview of what the name
  does.

### Changed

- Settings looks quieter: the sidebar uses plain monochrome symbols and setting rows no longer carry coloured icons.
- In English, Mo's spoken lines call it "Moss". Chinese stays 小莫.

### Fixed

- Opening a session when the terminal isn't running no longer leaves two windows: the session runs in the window the
  terminal starts with.
- Mo's message bubble stays above other floating windows and the Dock.
- Switching the language during the welcome no longer pops up a closed Settings window.
- The Accessibility step no longer gets stuck after an update. Each update looked like a new app to macOS, so the old
  permission stopped working while System Settings still showed it ticked; cc-moss now clears its stale entry before
  asking again, and you tick it once more.
- The short-name preview no longer jumps while it plays.

## [0.2.0] - 2026-10-17

The menubar app: on a Mac, your Claude Code sessions are now one glance away.

### Added

- **The menubar app** (macOS 14 or newer). A native app in the menu bar that reads and writes everything through the
  `cc-moss` command and never touches the registry itself.
  - **An icon that moves with your sessions**: working, waiting, needs you, stopped. Settings › Menu Bar can show a
    number beside it, and with Motion off it stays still and only changes when the state does.
  - **A Raycast-style panel** (click the icon, or tap Control twice from anywhere): type to search, Tab to filter by state, sessions
    grouped by project with worktree labels. Return opens a session in a new window of your terminal: Terminal by
    default, Ghostty recommended, iTerm2, WezTerm, Kitty and Alacritty too; ⇧⌘C copies the command for any other.
    Stopped sessions come back one by one or all at once, and older ones are folded away. New session, notes, send a
    message, rename and stop reminding are all there, and ⌘K lists every action with its shortcut.
  - **Alerts in one place**: a session needs you, stopped, or is back (and, if you ask, finished a turn), told by
    Notification Center or by Mo, never both. The same alert within 30 seconds counts once, and nothing pops up while
    the panel is open. A banner can open the session or bring it back.
  - **A startup reminder**: sessions that stopped while your Mac restarted, or while the app was away, are reported
    as soon as it starts.
  - **Mo, the desktop companion** (off until you switch it on): a small graphite pebble whose face shows how your
    sessions are doing. Three interaction modes (quiet, everyday, active) and three sizes; click it for the panel, drag
    it anywhere (it snaps to screen edges), right-click to keep it quiet for an hour or for the day, tuck it by the edge
    or send it off. Optional voice: a chime, or the message title spoken by macOS's own speech synthesis, at most once
    every 30 seconds.
  - **Settings** in six pages (General, Menu Bar, Mo, Notifications, Aliases, About) with search: Open at Login, the
    global shortcut (tap Control twice, or a key combination), a workspace where new sessions start, the terminal, the interface language (shared with the CLI), software update,
    and "Versions differ" when the app and the CLI are not the same version.
  - **A first-run welcome** in six skippable steps: set up the terminal side in one click, Open at Login and
    notifications, your terminal, Mo, and the shortcut. Settings › About shows it again.

- The app ships with cc-moss: same version, same release. The Homebrew formula and the macOS release archives
  carry `cc-moss.app` (ad-hoc signed); `cc-moss install` asks, then copies it to `~/Applications`, where macOS
  delivers its notifications (`--skip app` leaves it out). `cc-moss update` brings the app to the new version and
  restarts it if it was running, and `cc-moss doctor` reports its version and where it lives. Replacing the app is
  all or nothing (a failure keeps the old one), it is never downgraded, and under sudo it is left alone.
  `cc-moss install --uninstall` removes it too, with its "Open at Login" launch agent. Linux keeps the terminal
  edition.

- Native aliases: `cc-moss alias add tn new` makes `tn` work in every shell right away. Aliases are links in
  `~/.local/bin`, never go through a shell, and can be managed from the menubar app's Settings › Aliases.
  `doctor` reports broken or hidden ones; `doctor --fix` relinks them.

- Session events: two new hooks (`Notification`, `PostToolUse`) tell cc-moss when a session is waiting for your
  permission or answer. `cc-moss events [--json] [--since SEQ] [--limit N]` lists what just happened (kept for 7 days,
  never your prompts), and `ls --json` gains `attention`. Coming from 0.1.x, `doctor` reports the 2 missing hooks and
  `cc-moss setup` adds just those.

- `cc-moss send NAME -- TEXT` types a message into a running session, as if you typed it in its terminal; several
  lines arrive whole.

- `cc-moss config get|set ui.language` reads or changes the interface language without opening the file; comments
  and everything else in `config.toml` stay as they were.

- JSON for scripts and the app: `restore --yes --json` (with stable reason codes for failures),
  `install --dry-run --json`, `update --check --json`, `alias ls|add|rm --json`, `config get|set --json`, and
  `state_dir` and per-session `worktree` in `ls --json`.

### Changed

- `t` is now a native alias instead of a zsh function, so bash and fish users get it too. `setup` and `install`
  take the old function out of `.zshrc` (only cc-moss's own block).
- `cc-moss new` takes Claude Code's flags directly (`cc-moss new api --model opus`); the `--` before them is now
  optional.
- `cc-moss note` clears a field given as an empty string instead of saving it blank.

## [0.1.1] - 2026-10-16

### Changed

- The picker's menu, note box and "end it?" question open right under the session card, where "↵ Open  → More"
  was, instead of at the bottom of a tall terminal.
- Dependencies: ratatui 0.30, crossterm 0.29, rusqlite 0.40, toml 1, ulid 3.

### Fixed

- "Last message" no longer shows Claude Code's own markup (`<cross-session-message …>`, `<command-name>`) for
  messages from other sessions and slash commands: it shows the words inside, or nothing.

## [0.1.0] - 2026-10-16

The first release: Claude Code sessions that survive crashes, reboots and closed tabs.

### Added

- **Session registry.** Claude Code hooks (start, prompt, stop, end) record every interactive session in a local
  SQLite registry: session id, folder, launch arguments, process and tmux session. Sessions that were already running
  before setup are adopted the next time you look.
- **`cc-moss restore`.** Brings every interrupted session back in its own tmux session with `claude --resume`, in its
  original folder, with `--model`, `--permission-mode`, `--effort`, `--add-dir`, `--agent` and
  `--dangerously-skip-permissions` as they were. Anything that looks like a secret is dropped. A Claude Code that
  exits right after starting is reported, not counted as restored. `--dry-run` and `--yes` are supported.
- **`cc-moss new [NAME]`.** One name, one session: jumps into a running session, offers to restore an interrupted one,
  or starts a new one. `/rename` inside Claude Code renames the tmux session too.
- **Notes.** `cc-moss note` records a session's goal and where it stopped.
- **The picker** (`t`, or plain `cc-moss`; `t api` opens it already searching). Type to search or name a new session, arrows and Enter to open, → for the
  session menu, mouse support. Marks for working, waiting, interrupted and plain tmux sessions, plus git worktree
  labels.
- **`cc-moss install` and `cc-moss doctor`.** Set up hooks, a tuned tmux config, a starship prompt, shell fixes and
  optional aliases in marked blocks with timestamped backups. Missing tools are installed with Homebrew or apt, after
  asking. `doctor` checks every piece and says how to fix it. `install --uninstall` takes it all out again. On a
  login shell other than zsh, the zsh parts are skipped and your shell files are left alone.
- **`cc-moss setup` / `setup --uninstall`.** Hooks and the `t` shortcut only.
- **`cc-moss update`.** Checks for a new version and, after asking, updates the way cc-moss was installed:
  `brew upgrade`, `cargo install`, or for a release download, a SHA-256-verified, test-run, atomic swap of the binary
  that leaves the old one untouched if anything fails. Once a day the picker (top line, "Update to X.Y.Z") and
  `doctor` mention a new version; one HTTPS request to the GitHub releases page, never from the hooks. Turn it off
  with `[update] check = false` or `CC_MOSS_NO_UPDATE_CHECK=1`.
- **`cc-moss ls`** (with `--json` for scripts), **`attach`** and **`close`**.
- **English and Simplified Chinese**, picked from the system language (`CC_MOSS_LANG` or `[ui] language` overrides it).
- **Built to stay out of the way.** The hook never blocks a prompt, never exits non-zero and does nothing if the
  cc-moss binary is gone. State files are private to you. An older cc-moss leaves a newer registry alone.
- **The Continuum mark**: a coral ∞ in the README, the picker and `cc-moss --version`.
- Prebuilt binaries for macOS (Apple silicon, Intel) and Linux (x86_64, arm64, static musl), plus a Homebrew formula.

[Unreleased]: https://github.com/larrykoo711/cc-moss/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/larrykoo711/cc-moss/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/larrykoo711/cc-moss/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/larrykoo711/cc-moss/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/larrykoo711/cc-moss/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/larrykoo711/cc-moss/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/larrykoo711/cc-moss/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/larrykoo711/cc-moss/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/larrykoo711/cc-moss/releases/tag/v0.1.0
