# 003 · Site: search, AI answer engines and agents

Make cc-moss.larrykoo.com findable by search engines, quotable by AI answer engines, and usable by
agents (MCP). Patterns follow two sibling sites already in production (a marketing site and a docs
site on Cloudflare Workers); requirements come from the seo-geo / ai-seo / seo-audit checklists.

## Decisions (approved 2026-10-17)

- Hosting moves from GitHub Pages to a Cloudflare Worker with static assets: negotiation, Link
  headers and a POST endpoint need a server. The old github.io URL keeps redirecting.
- The zone's managed robots.txt is switched off; every site on the zone declares its own.
- One URL per language: English at `/`, Chinese at `/zh/`. The URL alone decides what is served;
  a first-time visitor whose browser prefers the other language is moved once, on the client.
- Nothing is declared that does not exist: no ratings, no API catalog (there is no API), no OAuth.
- AMEND (user, 2026-10-17): no guide page for people — the site is the app's page plus a plain FAQ.
  The full guide stays available to agents only, as Markdown (`/guide/index.md`, `/zh/guide/index.md`):
  in llms.txt, MCP search and read_doc, llms-full; not in the sitemap, no HTML; `/guide/` sends
  people to the FAQ.
- The zone's AI-bot blocking (`ai_bots_protection`) is off as well as its managed robots.txt: it
  answered AI fetchers with 403 before the Worker could serve them.
- `get_release` reads GitHub's releases feed: the REST API rate-limits Cloudflare's shared addresses.

## Stage 1: Worker hosting and one page per language
**Goal**: the site builds into `/` and `/zh/`, served by a Worker.
**Success Criteria**: each page holds only its own language; switching language navigates without a
jolt (cross-document view transition, scroll kept); `wrangler dev` serves all four.
**Tests**: build output has no `lang="zh"` text on `/` and no `lang="en"` text on `/zh/`.
**Status**: Complete

## Stage 2: Search basics
**Goal**: robots.txt, sitemap with hreflang and real lastmod, canonical/hreflang tags, JSON-LD
(SoftwareApplication, Organization, WebSite, FAQPage from a visible FAQ, BreadcrumbList on the guide),
1200×630 share images.
**Success Criteria**: seo-geo audit script passes; sitemap URLs all 200.
**Tests**: unit tests on the generated sitemap, robots and JSON-LD; FAQ JSON-LD equals the visible FAQ.
**Status**: Complete

## Stage 3: Agent-readable layer
**Goal**: llms.txt (with "when to use"), llms-full.txt, a `.md` twin for every page, `Accept:
text/markdown` negotiation, `Vary`, Link headers, a Markdown 404.
**Success Criteria**: `curl -H 'Accept: text/markdown'` on every page → 200 `text/markdown`.
**Tests**: negotiation unit tests (q-values, ties, 406, AI user agents); smoke script.
**Status**: Complete

## Stage 4: MCP and discovery
**Goal**: `POST /mcp` (stateless Streamable HTTP, read-only, no auth) with `search_docs`, `read_doc`,
`get_release`; server card, `.well-known/mcp.json`, `server.json`, agent-skills index; WebMCP.
**Success Criteria**: an MCP client lists and calls all three tools against production.
**Tests**: dispatcher unit tests (initialize, tools/list, tools/call, errors); smoke.
**Status**: Complete

## Stage 5: Cutover and grading
**Goal**: custom domain on the Worker, GitHub Pages retired, managed robots off, scanners run.
**Success Criteria**: isitagentready / is-agentic / ora results recorded here; every link in llms.txt
and the discovery files returns 200.
**Status**: Complete

## Results (2026-10-17, production)

- Smoke (`scripts/smoke.mjs https://cc-moss.larrykoo.com`): 46/46.
- Unit + build tests (`pnpm test`): 31/31.
- isitagentready: Level 4 "Agent-Integrated". Pass: robots, sitemap, Link headers, Markdown
  negotiation, AI rules, Content-Signal, MCP server card, agent skills, WebMCP (3 tools), ARD.
  Fail by design (nothing real to declare): API catalog, OAuth, auth.md, A2A, PAP. Open: DNS-AID
  (needs an SVCB record and DNSSEC on the zone).
- is-agentic: 100. Open: brand-search-accuracy (new domain), trust-anchors (wants /about, /contact,
  /privacy pages), org-schema-completeness (wants contact email and address; we publish none).
- seo-geo audit: title 53 / 38 chars, description 158 / 89 chars, one H1, JSON-LD, load 0.50 s / 0.69 s.

## Follow-ups (not in this iteration)

- Submit the sitemap to Google Search Console and Bing Webmaster Tools (needs the owner's accounts).
- DNS-AID record for the MCP server, if DNSSEC is turned on for the zone.
- GitHub Pages stays enabled only so the old github.io address keeps redirecting here.
