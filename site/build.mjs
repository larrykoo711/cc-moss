// Builds the site into dist/: one page per language, a Markdown twin of every page, the guide as
// Markdown for agents, and the files search engines and agents read. worker/ serves dist/.
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { discoveryFiles, llmsFull, llmsTxt, searchRecords } from "./lib/agent.mjs";
import { guideImages, guideMarkdown } from "./lib/guide.mjs";
import { assemble, localize, pageMarkdown, serialize, visibleFaq, withFrontmatter } from "./lib/pages.mjs";
import { headTags, robots, sitemap, twinPath } from "./lib/seo.mjs";
import { AGENT_DOCS, HTML_LANG, LOCALES, PAGES, lastModified, localePath, url } from "./lib/site.mjs";

const out = (path, content) => {
  const file = `dist/${path.replace(/^\//, "")}`;
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, content);
};

rmSync("dist", { recursive: true, force: true });
mkdirSync("dist");
execFileSync("node_modules/.bin/tailwindcss", ["-i", "src/styles.css", "-o", "dist/styles.css", "--minify"], { stdio: "inherit" });

// The ∞ is the brand logo's own path, read from the repo's logo.
const continuum = readFileSync("../docs/assets/logo.svg", "utf8").match(/<path d="([^"]+)"/)[1];
const partial = (name) => readFileSync(`src/partials/${name}.html`, "utf8").replaceAll("__CONTINUUM__", continuum);
const slots = { head: partial("head"), sprites: partial("sprites"), nav: partial("nav"), footer: partial("footer") };
const meta = (doc, locale, path, updated) => ({
  title: doc.title[locale],
  description: doc.description[locale],
  canonical: url(path),
  "last-updated": updated,
  lang: HTML_LANG[locale],
});

const docs = [];
const entries = [];
const images = new Set();

// Pages people see: HTML, plus the same content as Markdown.
for (const page of PAGES) {
  for (const locale of LOCALES) {
    const path = localePath(locale, page.path);
    const updated = lastModified(page.sources);
    const tokens = {
      lang: locale,
      html_lang: HTML_LANG[locale],
      home: localePath(locale, "/"),
      url_en: localePath("en", page.path),
      url_zh: localePath("zh", page.path),
    };
    const document = localize(assemble(page.template, slots, tokens).replaceAll("__CONTINUUM__", continuum), locale);
    const faq = visibleFaq(document);
    out(`${path}index.html`, serialize(document).replace("<!--@seo-->", headTags({ page, locale, updated, faq })));
    const markdown = withFrontmatter(meta(page, locale, path, updated), pageMarkdown(document, url(path)));
    out(twinPath(path), markdown);
    const ids = new Set([...document.querySelectorAll("[id]")].map((el) => el.id));
    docs.push({ locale, path, title: "cc-moss", markdown, ids });
    entries.push({ page, locale, updated });
  }
}

// Read by agents only: the full guide as Markdown, with its pictures served by the site.
for (const doc of AGENT_DOCS) {
  for (const locale of LOCALES) {
    const path = localePath(locale, doc.path);
    const source = doc.source[locale];
    const updated = lastModified([source]);
    const body = guideMarkdown(source);
    for (const img of guideImages(body)) images.add(img);
    // Read away from the site, so pictures need their full address.
    const markdown = withFrontmatter(meta(doc, locale, path, updated), body.replaceAll('src="/docs-assets/', `src="${url("/docs-assets/")}`));
    out(twinPath(path), markdown);
    docs.push({ locale, path, title: doc.title[locale], markdown, agentOnly: true });
  }
}

const siteUpdated = docs.map((d) => d.markdown.match(/last-updated: "([^"]+)"/)[1]).sort().at(-1);
out("sitemap.xml", sitemap(entries));
out("robots.txt", robots());
for (const locale of LOCALES) {
  out(localePath(locale, "/llms.txt"), llmsTxt(locale, siteUpdated));
  out(localePath(locale, "/llms-full.txt"), llmsFull(docs.filter((d) => d.locale === locale)));
}
out("search-records.json", JSON.stringify(searchRecords(docs)));
for (const [path, content] of Object.entries(discoveryFiles())) out(path, content);

// A 404 that helps: where to go, for people and for agents.
const notFound = assemble("src/404.template.html", slots, { lang: "en", html_lang: "en", home: "/", url_en: "/", url_zh: "/zh/" }).replaceAll(
  "__CONTINUUM__",
  continuum,
);
out("404.html", serialize(localize(notFound, "en")).replace("<!--@seo-->", '  <title>Page not found · cc-moss</title>\n  <meta name="robots" content="noindex">'));
out(
  "404.md",
  `# Page not found\n\nThere is no page at this address. Try:\n\n- [Overview and FAQ](${url("/index.md")})\n- [Full guide](${url("/guide/index.md")})\n- [All pages for AI assistants](${url("/llms.txt")})\n- [Sitemap](${url("/sitemap.xml")})\n`,
);

for (const f of ["app.js", "favicon.svg"]) cpSync(`src/${f}`, `dist/${f}`);
cpSync("assets", "dist/assets", { recursive: true });
for (const img of images) cpSync(`../docs/assets/${img}`, `dist/docs-assets/${img}`);
if (existsSync("og")) cpSync("og", "dist/og", { recursive: true });
// The app icon, for the nav and the notification.
cpSync("../docs/assets/icon.svg", "dist/icon.svg");
console.log(`built dist/: ${entries.length} pages, ${docs.length} Markdown documents`);
