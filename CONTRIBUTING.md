# Contributing to cc-moss

**English** · [简体中文](CONTRIBUTING.zh-CN.md)

Thanks for stopping by. cc-moss exists so that a crash never costs you a conversation, and contributions that make that
promise sturdier are very welcome: bug reports, docs, translations, tests and code.

## Before you start

- **Bugs:** open an issue with the bug form. `cc-moss doctor` output answers half our questions before we ask them.
- **Features:** open a feature request first if the change is bigger than a few lines, so nobody builds something twice.
- **Security problems:** please don't open an issue. See [SECURITY.md](SECURITY.md).

## Development setup

You need a Rust toolchain (stable; see `rust-version` in `Cargo.toml` for the minimum), `tmux`, and a POSIX shell with
`bash`. `zsh` is only needed to syntax-check the shell assets.

```sh
git clone https://github.com/larrykoo711/cc-moss
cd cc-moss
cargo test --all
```

The end-to-end tests start real tmux servers on private sockets with a throwaway `HOME` and a fake `claude`. They
never touch your running tmux server or `~/.claude`, so it is safe to run them on the machine you work on.

Before you open a pull request, run what CI runs:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
for f in crates/cc-moss/assets/zsh/*.zsh; do zsh -n "$f"; done
```

The hook latency test is opt-in: `cargo test --release -p cc-moss --test hook_perf -- --ignored`.
`scripts/clean-room.sh <linux binary>` replays `install` and `doctor` on a fresh Ubuntu in Docker.

## How the code is laid out

| Path | What lives there |
|------|------------------|
| `crates/moss-core` | Registry (SQLite), hooks, tmux adapter, restore planning |
| `crates/cc-moss` | The `cc-moss` binary, the picker and the shell/tmux/starship assets |
| `docs/assets` | README images |
| `.sdd/` | Design notes, plans and specs (in Chinese). Not required reading. |

## House rules

- **Two languages.** Every user-facing string has an English and a Simplified Chinese version (the `tr!(en, zh)`
  macro). README changes go into both `README.md` and `README.zh-CN.md`. If you only speak one of them, say so in the
  PR and we will fill in the other.
- **Tests come with the change.** Behaviour changes need a test, and bug fixes need a test that failed before the fix.
- **Never touch the real world in tests.** Use the isolated tmux socket and temporary `HOME` helpers in `tests/common`.
- **Nothing private.** No hostnames, IPs, account names, tokens or absolute home paths, in code, tests, docs or `.sdd/`.
  Use placeholders like `host-a` and `192.0.2.10`.
- **No network.** cc-moss itself never talks to the network. `install` may run a package manager after asking.
- **Commits.** English [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`,
  `test:`, `refactor:`, `chore:`. One subject line, then bullet points if needed. PRs are squash-merged, so the PR title
  becomes the commit; make it a good one.
- **AI-assisted contributions are welcome.** This is a Claude Code tool, after all. You are still the author: read the
  diff, run the checks and be ready to explain it.

## Releasing (maintainers)

1. Move `Unreleased` entries in `CHANGELOG.md` under the new version and bump `version` in `Cargo.toml`.
2. Merge, then tag: `git tag v0.2.0 && git push origin v0.2.0`.
3. `release.yml` builds the binaries, publishes the GitHub Release with the changelog section and bumps the Homebrew
   formula.

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
