import Foundation

// Synthetic /usage and /status screens laid out as Claude Code 2.1 draws
// them in a 100-column terminal. Every value is made up.

let usageScreen = """
────────────────────────────────────────────────────────────────────────────────────────────────────
  Settings  Status   Config   Usage   Stats

  Session

  Total cost:            $1.2400
  Total duration (API):  3m 12s
  Total duration (wall): 41m 5s
  Total code changes:    120 lines added, 8 lines removed
  Usage:                 18.2k input, 4.1k output, 210k cache read, 12k cache write

  Current session
  ██████▌                                            13% used
  Resets 1:49pm (Europe/Berlin)

  Current week (all models)
  ██████████████████████████                         52% used
  Resets Oct 23 at 12:59am (Europe/Berlin)

  Current week (Sonnet)
                                                     0% used
  Resets Oct 23 at 1am (America/Los_Angeles)

  What's contributing to your limits usage?
  Approximate, based on local sessions on this machine — does not include other devices or
  claude.ai
  Last 24h · these are independent characteristics of your usage, not a breakdown

  98% of your usage came from subagent-heavy sessions
   Each subagent runs its own requests. Be deliberate about spawning them — and
   consider configuring a cheaper model for simpler subagents.

  85% of your usage was at >150k context
   Longer sessions are more expensive even when cached. /compact mid-task, /clear
   when switching to new tasks.

  16% of your usage was while 4+ sessions ran in parallel
   All sessions share one limit.

  7% of your usage came from something new
   A hint nobody translated.

  Skills                  % of usage
  /review                         1%
  /deploy                         3%

  Subagents               % of usage
  general-purpose                38%
  Explore                         1%

  MCP servers             % of usage
  browser-tools                   3%

  Loops                                      every  runs  tokens  per run last run
  nightly report check (once a day, rea…   at 10:17     1  206.7k   206.7k   1h ago

  d to day · w to week

  Usage credits
  Usage credits are off · /usage-credits to turn them on

  Esc to cancel
"""

let statusScreen = """
────────────────────────────────────────────────────────────────────────────────────────────────────
  Settings  Status   Config   Usage   Stats

  Version:           2.1.303
  Session name:      /rename to add a name
  Session ID:        0f3c9a52-1b7e-4d0a-9c11-5e2f7a8b9c0d
  Session kind:      interactive
  Peer address:      uds:/tmp/example.sock
  cwd:               /Users/alex/work/projects/a-very-long-folder-name-that-the-terminal-will-wrap-somewh
                     ere-in-the-middle/app
  Login method:      Claude Max account
  Organization:      Example Org
  Email:             alex@example.com
  Cloud sessions:    GitHub connected
  Model:             Default (Opus 5.5 · Best for everyday, complex tasks)
  IDE:               ✘ Not connected to Visual Studio Code
  MCP servers:       1 disabled · /mcp
  Setting sources:
  Auto mode server:  Disabled

  Esc to cancel
"""
