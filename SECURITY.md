# Security policy

## Supported versions

cc-moss is young. Security fixes land in the latest release only.

| Version | Supported |
|---------|-----------|
| latest release | yes |
| anything older | please upgrade |

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Report privately through GitHub:
[**Report a vulnerability**](https://github.com/larrykoo711/cc-moss/security/advisories/new)
(Security tab → Advisories → Report a vulnerability).

Helpful details: the cc-moss version (`cc-moss --version`), your OS, what an attacker can do, and steps to reproduce.

This is a one-maintainer project, so here is what you can honestly expect:

- an acknowledgement within 7 days,
- an assessment and a plan within 14 days,
- a fix and a published advisory crediting you (unless you prefer otherwise) as soon as it is ready.

## What is in scope

cc-moss runs on your machine with your permissions. It edits `~/.claude/settings.json`, your shell rc files and tmux
config, keeps a SQLite registry under `~/.local/state/cc-moss`, records Claude Code launch arguments, and can run package
managers and starship's installer when you agree. Issues we would treat as security bugs include:

- secrets from launch arguments ending up in the registry, `ls --json` or tmux (cc-moss is meant to drop them),
- writing files with wider permissions than the original, or following symlinks somewhere unexpected,
- command injection through session names, notes, folders or hook payloads,
- `install` running anything without showing it first, or fetching over plain HTTP.

Claude Code itself, tmux and starship are out of scope. Please report those upstream.
