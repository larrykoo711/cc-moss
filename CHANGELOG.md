# Changelog

All notable changes to cc-moss are recorded here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, minor versions may change behaviour.

## [Unreleased]

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

[Unreleased]: https://github.com/larrykoo711/cc-moss/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/larrykoo711/cc-moss/releases/tag/v0.1.0
