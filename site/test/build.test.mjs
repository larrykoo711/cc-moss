// Checks the built site (run `pnpm build` first): every surface that lists pages lists the same
// pages, every link a machine file hands out leads somewhere, and nothing claims what isn't shown.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";

const ORIGIN = "https://cc-moss.larrykoo.com";
const read = (p) => readFileSync(`dist/${p.replace(/^\//, "")}`, "utf8");
const PAGES = ["/", "/zh/", "/changelog/", "/zh/changelog/"];
const AGENT_DOCS = ["/guide/", "/zh/guide/"];

/** A site URL as the file the Worker would serve for it. */
function fileFor(href) {
  const path = new URL(href, ORIGIN).pathname;
  if (path === "/mcp") return "worker";
  if (path.endsWith("/")) return `${path}index.html`;
  return path;
}

test("every page has its HTML and its Markdown twin; the guide is Markdown only", () => {
  const front = /^---\ntitle: .*\ndescription: .*\ncanonical: "https:\/\/cc-moss\.larrykoo\.com\//;
  for (const p of PAGES) {
    assert.ok(existsSync(`dist${p}index.html`), `${p} html`);
    assert.match(read(`${p}index.md`), front);
  }
  for (const p of AGENT_DOCS) {
    assert.ok(!existsSync(`dist${p}index.html`), `${p} has no page for people`);
    assert.match(read(`${p}index.md`), front);
    for (const m of read(`${p}index.md`).matchAll(/src="https:\/\/cc-moss\.larrykoo\.com(\/docs-assets\/[^"]+)"/g))
      assert.ok(existsSync(`dist${m[1]}`), m[1]);
  }
});

test("the sitemap lists exactly the pages, each with its language alternates", () => {
  const xml = read("sitemap.xml");
  const locs = [...xml.matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => m[1].replace(ORIGIN, ""));
  assert.deepEqual(locs.sort(), [...PAGES].sort());
  assert.equal((xml.match(/hreflang="x-default"/g) || []).length, PAGES.length);
  for (const m of xml.matchAll(/<lastmod>([^<]+)<\/lastmod>/g)) assert.match(m[1], /^\d{4}-\d{2}-\d{2}$/);
});

test("llms.txt links every page's Markdown, and every link it gives resolves", () => {
  for (const file of ["llms.txt", "zh/llms.txt"]) {
    const txt = read(file);
    assert.match(txt, /^# cc-moss\n\n> /);
    const links = [...txt.matchAll(/\]\((https:\/\/cc-moss\.larrykoo\.com[^)]*)\)/g)].map((m) => m[1]);
    const zh = file.startsWith("zh");
    for (const p of [...PAGES, ...AGENT_DOCS].filter((p) => p.startsWith("/zh/") === zh))
      assert.ok(links.includes(`${ORIGIN}${p}index.md`), `${file} lists ${p}index.md`);
    for (const link of links) {
      const f = fileFor(link);
      if (f !== "worker") assert.ok(existsSync(`dist${f}`), `${file}: ${link} → dist${f}`);
    }
  }
});

test("each page holds only its own language", () => {
  const visible = (p) => read(p).replace(/<head>[\s\S]*?<\/head>|<script[\s\S]*?<\/script>|<svg[\s\S]*?<\/svg>/g, "").replace(/<[^>]+>/g, " ");
  for (const p of ["/index.html", "/changelog/index.html"]) {
    const cjk = visible(p).match(/[一-鿿]+/g) || [];
    assert.deepEqual(cjk, ["中"], `${p}: only the language switch is Chinese`);
  }
  assert.doesNotMatch(visible("/zh/index.html"), /Questions people ask|Every session\./);
  assert.doesNotMatch(visible("/zh/changelog/index.html"), /Latest|version by version/);
});

test("canonical and hreflang agree on every page", () => {
  for (const p of PAGES) {
    const html = read(`${p}index.html`);
    assert.ok(html.includes(`<link rel="canonical" href="${ORIGIN}${p}">`), `${p} canonical`);
    const alts = [...html.matchAll(/hreflang="([^"]+)" href="([^"]+)"/g)].map((m) => `${m[1]}=${m[2].replace(ORIGIN, "")}`);
    const base = p.replace(/^\/zh\//, "/");
    assert.deepEqual(alts.sort(), [`en=${base}`, `x-default=${base}`, `zh=/zh${base}`].sort(), p);
  }
});

test("FAQPage structured data is the FAQ the page shows", () => {
  for (const p of ["/", "/zh/"]) {
    const html = read(`${p}index.html`);
    const graph = JSON.parse(html.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)[1])["@graph"];
    const faq = graph.find((n) => n["@type"] === "FAQPage");
    const shown = [...html.matchAll(/<h3 class="faq-q[^"]*">([\s\S]*?)<\/h3>/g)].map((m) => m[1].replace(/<[^>]+>/g, "").trim());
    assert.equal(shown.length, 13);
    assert.deepEqual(faq.mainEntity.map((q) => q.name), shown);
    assert.ok(graph.some((n) => n["@type"] === "SoftwareApplication" && n.offers.price === "0"));
    assert.ok(!JSON.stringify(graph).includes("aggregateRating"), "no ratings we don't have");
  }
});

test("robots.txt welcomes crawlers, states the training preference and names the sitemap", () => {
  const txt = read("robots.txt");
  assert.match(txt, /^User-agent: \*$/m);
  assert.match(txt, /^Allow: \/$/m);
  assert.match(txt, /^Content-Signal: .*ai-train=no/m);
  assert.match(txt, /^Sitemap: https:\/\/cc-moss\.larrykoo\.com\/sitemap\.xml$/m);
  assert.doesNotMatch(txt, /Disallow: \/$/m);
});

test("the agent skill's digest is the digest of what is served", () => {
  const index = JSON.parse(read(".well-known/agent-skills/index.json"));
  const skill = read(".well-known/agent-skills/cc-moss/SKILL.md");
  assert.equal(index.skills[0].digest, `sha256:${createHash("sha256").update(skill).digest("hex")}`);
});

test("discovery files point at the same server and at files that exist", () => {
  const card = JSON.parse(read(".well-known/mcp/server-card.json"));
  const manifest = JSON.parse(read(".well-known/mcp.json"));
  const registry = JSON.parse(read("server.json"));
  assert.equal(card.remotes[0].url, `${ORIGIN}/mcp`);
  assert.equal(manifest.url, `${ORIGIN}/mcp`);
  assert.equal(registry.remotes[0].url, `${ORIGIN}/mcp`);
  assert.deepEqual(manifest.tools.map((t) => t.name), card.tools.map((t) => t.name));
  const catalog = JSON.parse(read(".well-known/ai-catalog.json"));
  for (const e of catalog.entries) assert.ok(existsSync(`dist${fileFor(e.url)}`), e.url);
});

test("search records cover both languages and both pages, and every anchor exists", () => {
  const records = JSON.parse(read("search-records.json"));
  for (const r of records.filter((r) => r.url.includes("#") && !r.url.includes(".md"))) {
    const [page, anchor] = r.url.replace(ORIGIN, "").split("#");
    assert.ok(read(`${page}index.html`).includes(`id="${anchor}"`), r.url);
  }
  for (const locale of ["en", "zh"])
    for (const page of ["/", "/changelog/", "/guide/"]) {
      const prefix = `${ORIGIN}${locale === "zh" ? "/zh" : ""}${page}`;
      assert.ok(records.some((r) => r.locale === locale && r.url.startsWith(prefix)), `${locale} ${page}`);
    }
  // Guide sections link to the guide's Markdown: there is no page for people to land on.
  assert.ok(records.filter((r) => r.url.includes("/guide/")).every((r) => r.url.endsWith("/guide/index.md")));
});

test("both language links are visible on both pages", () => {
  assert.doesNotMatch(read("styles.css"), /\[lang=["']?en["']?\][^{]*\{[^}]*display:\s*none/);
  for (const p of PAGES) {
    const html = read(`${p}index.html`);
    assert.match(html, /data-set-lang="zh"/);
    assert.match(html, /data-set-lang="en"/);
  }
});

test("every link and picture on a page resolves to a file the site serves", () => {
  for (const p of PAGES) {
    const html = read(`${p}index.html`).replace(/<script[\s\S]*?<\/script>/g, "");
    for (const m of html.matchAll(/\s(?:src|href)="([^"]+)"/g)) {
      const ref = m[1];
      if (/^(https?:|mailto:|#|data:)/.test(ref)) continue;
      const path = new URL(ref, `${ORIGIN}${p}`).pathname;
      const file = path.endsWith("/") ? `${path}index.html` : path;
      assert.ok(existsSync(`dist${file}`), `${p}: ${ref} → dist${file}`);
    }
  }
});

test("the Changelog page has a card for every release, newest first, and links to the full notes", () => {
  const versions = JSON.parse(readFileSync("src/changelog.json", "utf8")).versions.map((v) => v.version);
  for (const p of ["/changelog/", "/zh/changelog/"]) {
    const html = read(`${p}index.html`);
    const ids = [...html.matchAll(/<h2 id="(v\d+-\d+-\d+)"/g)].map((m) => m[1]);
    assert.deepEqual(ids, versions.map((v) => `v${v.replaceAll(".", "-")}`), p);
    assert.match(html, /href="https:\/\/github\.com\/larrykoo711\/cc-moss\/blob\/main\/CHANGELOG\.md"/);
    assert.match(html, /data-nav="changelog"[^>]*aria-current="page"|aria-current="page"[^>]*data-nav="changelog"/);
    // Agents read the same releases, each under its own heading.
    const md = read(`${p}index.md`);
    for (const v of versions) assert.match(md, new RegExp(`^## ${v.replaceAll(".", "\\.")} · `, "m"), `${p}index.md: ${v}`);
  }
  // Search links each release to its card.
  const records = JSON.parse(read("search-records.json"));
  assert.ok(records.some((r) => r.url === `${ORIGIN}/changelog/#v${versions[0].replaceAll(".", "-")}`));
  // The home page's badge points at the newest card.
  assert.ok(read("/index.html").includes(`href="/changelog/#v${versions[0].replaceAll(".", "-")}"`));
  assert.ok(read("/zh/index.html").includes(`href="/zh/changelog/#v${versions[0].replaceAll(".", "-")}"`));
});
