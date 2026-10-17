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
| `site/` | The website at cc-moss.larrykoo.com: static pages built by `build.mjs` (one per language, plus Markdown for agents) served by a Cloudflare Worker (`worker/`: content negotiation, `/mcp`); deployed by `.github/workflows/site.yml` |
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

## User-facing copy and the website (settled 2026-10-17)

- Words people read in the app and on the site are written for everyday users: plain words, no jargon, sentence
  case in English, native English rather than a translation of the Chinese. The full rules and examples are in
  `.sdd/002-menubar/notes/design-voice.md`.
- The website shows people one page and a plain FAQ, nothing more. Detail for AI assistants lives where people
  don't see it: the full guide is served as Markdown only (`/guide/index.md`), listed in `llms.txt` and searchable
  over `/mcp`.
- Every claim on the site matches the guide's facts (privacy, network use, requirements). Check the guide before
  writing a sentence about what cc-moss does; the site once said "no network" while the version check goes online.
- Pages live at language subpaths (`/zh/`), so every asset and link in `site/src` is root-absolute (`/icon.svg`);
  a relative path breaks on one language only. `site/test/build.test.mjs` checks that every link resolves.
- Structured data is built from what the page shows (the FAQPage comes from the rendered FAQ) and never declares
  what doesn't exist: no ratings, no API catalog, no contact details we don't publish.
