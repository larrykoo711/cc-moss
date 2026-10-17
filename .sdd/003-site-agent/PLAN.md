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

## Kanban

| Task | 仓 | 依赖 | 状态 | SDD | 验收锚点 |
|------|----|------|------|-----|----------|
| **S1** Worker 托管与一页一语言 | cc-moss | — | ✅ Done | —（阶段计划，无逐任务 spec） | `/` 与 `/zh/` 只含本语言；切换保位 |
| **S2** 搜索基础 | cc-moss | S1 | ✅ Done | — | sitemap、hreflang、JSON-LD 来自可见 FAQ；seo-geo 审计 |
| **S3** Agent 可读层 | cc-moss | S1 | ✅ Done | — | Markdown 协商、Link、llms.txt、Markdown 404 |
| **S4** MCP 与发现文件 | cc-moss | S3 | ✅ Done | — | 线上三工具可调；server card、技能、WebMCP |
| **S5** 切换与评分 | cc-moss | S1–S4 | ✅ Done | — | 线上冒烟 46/46；isitagentready L4；is-agentic 100 |
| **F1** 反馈：手册改为 Agent 专用、FAQ 大白话 | cc-moss | S5 | ✅ Done | — | 人看不到 `/guide/`；FAQ 10 条 |
| **F2** 反馈：中文页通知图标裂图 | cc-moss | F1 | ✅ Done | — | 相对路径改绝对；链接可达测试（变异验证） |

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

## 日志

- 2026-10-17 用户：参照两个已在生产的兄弟站点（营销站、文档站）与 seo/geo/agent-readable skill 全面升级，含站点的 agent MCP 能力；三个决策（Workers 托管、zone 级关托管 robots、URL 决定语言 + 首访跳一次）均按推荐
- 2026-10-17 用户反馈：手册太技术，C 端只要 FAQ；手册放在用户看不到、agent 能读的地方 → F1
- 2026-10-17 用户「直接 PR 合并，我从线上查看」→ #24 合入，Site 工作流部署并线上冒烟通过
- 2026-10-17 用户反馈：中文页通知图标裂图 → F2；新增「每个链接与图片都能解析」构建测试，变异（改回相对路径）可被抓到
- DEVIATION: 本迭代按阶段计划直接实施，没有逐任务 spec、未过 sdd-gate；用户在过程中逐项定调与验收，如实记录
- ARCHIVE: 原地标记收尾，不移入 archive/（同 002，宿主约定）
E2E: assertion=site 32/32 + 线上冒烟 46/46 green, console clean feel=done（用户线上查看并反馈 F1、F2，均已修复）
DISTILL: CLAUDE.md items=5（与 002 共用一次沉淀，见 002 PLAN 同名行）
REPORT: playground=n/a（本迭代无 playground，报告即 reports/delivery.md）buckets=[delivered:7 new:2 sub:1 deferred:3]
