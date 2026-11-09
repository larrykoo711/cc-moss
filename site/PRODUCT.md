# Product

<!-- impeccable:product-schema 1 -->

Scope: the cc-moss website at cc-moss.larrykoo.com (this `site/` folder). The product it presents (the `cc-moss`
CLI, the macOS menu bar app, Mo and Moss) lives in the rest of this repo; facts about it come from `docs/guide.md`.

## Platform

web

## Users

People who keep several Claude Code sessions open at once, often from a few to a dozen in parallel, every day.
Most write code, but not all: some use Claude Code for other work and don't know tmux or the terminal well.
They reach the site from a link, a search or an AI assistant, and want to know in a minute what cc-moss does for
them, whether it's safe, and how to install it.

A second audience reads the site through machines: AI assistants and agents that fetch the Markdown pages,
`llms.txt`, the guide (`/guide/index.md`) or the `/mcp` endpoint to answer questions about cc-moss.

## Product Purpose

cc-moss looks after Claude Code sessions. It records every session as it runs; after a crash, a restart or a closed
terminal, each one comes back in its own folder and picks up the same conversation. On a Mac, a menu bar app shows
which sessions are busy, waiting or stopped and lets you answer the waiting ones without the terminal; Mo, a small
desktop companion, speaks up when a session needs you; Moss, a Claude Code session of its own, checks on the others
once a day and asks before it changes anything.

The website's job: explain that in plain words, show the real app, answer common questions, and get the visitor to
install it (`brew install larrykoo711/tap/cc-moss && cc-moss install`). Success is a visitor who installs it, or
one who leaves knowing it isn't for them.

## Positioning

Sessions don't get lost: after anything goes wrong, one click (or `cc-moss restore`) brings every session back,
in its own folder, with its conversation. Everything else (the panel, Mo, Moss, answering from the menu bar) grows
out of looking after sessions. When design choices compete, this promise comes first.

## Operating Context

- Pages: the home page (tour, restore story, Moss, features, Mo, terminal edition, comparison, FAQ, install) and the
  Changelog timeline (`/changelog/`). Nothing more for people (decided 2026-10-20).
- Two languages, each at its own URL: English at `/`, Chinese at `/zh/`. Chinese is the first language of the
  product; the English is written for English readers, not translated.
- Light and dark skins, following the system until the visitor picks one.
- Every page also exists as Markdown for agents; the full guide is served only as Markdown, listed in `llms.txt`
  and searchable over `/mcp`.

## Capabilities and Constraints

- Stack: static pages built by `build.mjs` from `src/*.template.html` with Tailwind CSS 4 and a small
  framework-free `src/app.js`; served by a Cloudflare Worker (`worker/`: content negotiation, `/mcp`); deployed
  by `.github/workflows/site.yml`. Tests: `pnpm test`; live check: `scripts/smoke.mjs`.
- Assets and links in `src` are root-absolute (`/icon.svg`); a relative path breaks one language only.
- Screenshots exist per language and per skin (`assets/<name>.<zh|en>.<light|dark>.webp`).
- Structured data is built from what the page shows (FAQPage comes from the rendered FAQ) and never declares what
  doesn't exist.
- Every claim matches the guide's facts on privacy, network use and requirements. The version check goes online,
  so the site never says "no network".
- Requirements to state truthfully: macOS 14 or later and Claude Code; Linux runs the terminal tool only, without
  the menu bar app or Moss.
- Everything here is public: no hostnames, private IPs, account names or credentials.

## Brand Commitments

- Name: cc-moss, after MOSS in *The Wandering Earth*, whose mission is to keep civilization going; cc-moss keeps
  conversations going. Chinese tagline: 「文明会延续，对话也是。」
- Fixed terms in both languages: 会话 / session, 面板 / panel, 小莫 / Mo, Moss, 找回 / bring back,
  断了 / stopped, 在等你 / waiting for you.
- Voice (full rules: `.sdd/002-menubar/notes/design-voice.md`): a reliable friend talking, not software reporting;
  honest before pleasing (say what is off by default, what goes online, what is slower); calm, no exclamation marks,
  no "oops"; plain labels, with the warmth left to Mo and the icons; "you", never "we"; no jargon (hooks, tmux,
  daemon, config) in words people read; English in sentence case for labels and copy, with contractions.
- Free and open source under MIT, at github.com/larrykoo711/cc-moss.

## Evidence on Hand

- Real app screenshots: `assets/` (panel, queue, archive, provider, Moss, Mo asking), per language and skin.
- README images rendered from demo data: `../docs/assets/`.
- The user guide (`../docs/guide.md`, `../docs/guide.en.md`) and the changelog entries (`src/changelog.json`).
- No testimonials, user counts, ratings, press or benchmarks exist. Do not invent them.

## Product Principles

1. The restore promise leads; every section earns its place by helping a visitor trust or try it.
2. Plain words for everyday people; detail for agents goes where people don't see it.
3. Say only what is true today and matches the guide; when unsure, leave it out.
4. Show the real app rather than describe it.
5. Calm and quiet by default, like the app itself.

## Accessibility & Inclusion

No standard has been set for the site. Existing behavior to keep: motion stops for visitors who prefer reduced
motion, and both languages and both skins carry the same content.
