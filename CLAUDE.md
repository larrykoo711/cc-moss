# cc-moss

Recoverable Claude Code sessions plus a terminal toolkit (tmux, starship, session tools), written in Rust.
Open source under `larrykoo711/cc-moss`.

The name comes from MOSS, the AI in *The Wandering Earth* whose mission is to keep human civilization going.
cc-moss does the same for your Claude Code sessions: crashes and reboots happen, the work carries on.

## Layout

| Path | Purpose |
|------|---------|
| `.sdd/` | Spec-driven development archive (config, per-iteration designs, plans, specs) |
| `.sdd/001-cli/` | Stage 1: terminal edition |

## Conventions

- Everything in this repo is public: no hostnames, private IPs, account names or credentials, including in `.sdd/`.
- Commits: English Conventional Commits, one subject line plus bullet points.
- End-to-end tests run against an isolated tmux socket and a temporary `HOME`, never the user's live tmux server or `~/.claude`.
