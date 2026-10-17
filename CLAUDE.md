# cc-moss

Recoverable Claude Code sessions plus a terminal toolkit (tmux, starship, session tools), written in Rust.
Open source under `larrykoo711/cc-moss`.

The name comes from MOSS, the AI in *The Wandering Earth* whose mission is to keep human civilization going.
cc-moss does the same for your Claude Code sessions: crashes and reboots happen, the work carries on.

## Layout

| Path | Purpose |
|------|---------|
| `crates/moss-core` | Library: registry, hooks, tmux, restore planning |
| `crates/cc-moss` | The `cc-moss` binary and the picker |
| `docs/assets/` | README images (banner, picker screenshots rendered from demo data) |
| `site/` | The landing page (Tailwind, `pnpm build`), published to GitHub Pages by `.github/workflows/pages.yml` |
| `.sdd/` | Spec-driven development archive (config, per-iteration designs, plans, specs) |
| `.sdd/001-cli/` | Stage 1: terminal edition |
| `packaging/homebrew/` | Homebrew formula template; `release.yml` renders it into `larrykoo711/homebrew-tap` |
| `.github/` | CI, release, issue forms |

## Conventions

- Everything in this repo is public: no hostnames, private IPs, account names or credentials, including in `.sdd/`.
- Commits: English Conventional Commits, one subject line plus bullet points.
- The README is Chinese first (`README.md`) with an English twin (`README.en.md`); both are short and friendly for everyday
  users. Commands, settings and internals go in the guide (`docs/guide.md`, `docs/guide.en.md`). Change both languages together.
- End-to-end tests run against an isolated tmux socket and a temporary `HOME`, never the user's live tmux server or `~/.claude`.
- Before committing, run what CI runs: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --all`.
- Contributor guide: `CONTRIBUTING.md` (English) and `CONTRIBUTING.zh-CN.md`.
