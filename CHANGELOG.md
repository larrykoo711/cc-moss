# Changelog

All notable changes to cc-moss are recorded here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, minor versions may change behaviour.

## [Unreleased]

### Added

- **A Permissions page in Settings**: Accessibility, notifications and control of your terminal (Automation), each
  with where it stands, what it's for and Allow when it's missing. A dot beside the page means one is off while
  something uses it. Pages that rely on a permission show the same row where it's missing.
- **Told when an update takes Accessibility away.** The panel says "Allow Accessibility again" at the top, with Allow,
  instead of the shortcut going quiet without a word.

### Fixed

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

[Unreleased]: https://github.com/larrykoo711/cc-moss/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/larrykoo711/cc-moss/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/larrykoo711/cc-moss/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/larrykoo711/cc-moss/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/larrykoo711/cc-moss/releases/tag/v0.1.0
