// Pages are written once with both languages side by side; the build cuts each language out
// into its own page, and turns the page into Markdown for agents.
import { readFileSync } from "node:fs";
import { parseHTML } from "linkedom";
import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";
import { ORIGIN } from "./site.mjs";

/** Fill `<!--@name-->` slots with partials and `{{token}}`s with values. */
export function assemble(file, slots, tokens) {
  let html = readFileSync(file, "utf8");
  for (let i = 0; i < 3 && /<!--@(\w+)-->/.test(html); i++)
    html = html.replace(/<!--@(\w+)-->/g, (m, name) => (name in slots ? slots[name] : m));
  return html.replace(/\{\{(\w+)\}\}/g, (m, name) => {
    if (!(name in tokens)) throw new Error(`${file}: no value for {{${name}}}`);
    return tokens[name];
  });
}

/** Keep one language: drop the other's text, pick its alt text, mark the root. */
export function localize(html, locale) {
  const { document } = parseHTML(html);
  const other = locale === "zh" ? "en" : "zh";
  for (const el of document.querySelectorAll(`[lang="${other}"]`)) if (!el.hasAttribute("data-set-lang")) el.remove();
  for (const el of document.querySelectorAll(`[lang="${locale}"]`)) {
    // The language is now the page's own; inline spans no longer need to say so.
    if (el.tagName === "SPAN" && !el.hasAttribute("data-set-lang")) el.removeAttribute("lang");
  }
  for (const img of document.querySelectorAll("[data-alt-en]")) {
    img.setAttribute("alt", img.getAttribute(`data-alt-${locale}`));
    img.removeAttribute("data-alt-en");
    img.removeAttribute("data-alt-zh");
  }
  return document;
}

export const serialize = (document) => `<!doctype html>\n${document.documentElement.outerHTML}\n`;

/** The FAQ exactly as the page shows it, for FAQPage structured data. */
export function visibleFaq(document) {
  return [...document.querySelectorAll(".faq-item")].map((item) => ({
    q: item.querySelector(".faq-q").textContent.trim(),
    a: item.querySelector(".faq-a").textContent.replace(/\s+/g, " ").trim(),
  }));
}

/** The page's own content as Markdown: no mockups, no buttons, links made absolute. */
export function pageMarkdown(document, pageUrl) {
  const main = document.querySelector("main").cloneNode(true);
  for (const el of main.querySelectorAll('[data-agent="skip"], script, svg, img, .copy-label, [aria-hidden="true"], .sr-only, #mode-caption, .mode'))
    el.remove();
  // A line break inside a heading is layout, not content.
  for (const br of main.querySelectorAll("h1 br, h2 br, h3 br")) br.replaceWith(document.createTextNode(" "));
  // Wrappers that exist for layout give their content back to the parent.
  for (const el of [...main.querySelectorAll('[data-agent="unwrap"]')].reverse()) el.replaceWith(...el.childNodes);
  // A row of chips reads as a list in prose.
  for (const el of main.querySelectorAll('[data-agent="join"]')) {
    const p = document.createElement("p");
    p.textContent = [...el.children].map((c) => c.textContent.trim()).join(", ");
    el.replaceWith(p);
  }
  for (const a of main.querySelectorAll("a[href]")) {
    const href = a.getAttribute("href");
    if (href.startsWith("#")) a.setAttribute("href", pageUrl + href);
    else if (href.startsWith("/")) a.setAttribute("href", ORIGIN + href);
  }
  // A copy button carries a command; keep it as code.
  for (const b of main.querySelectorAll("button[data-copy]")) {
    const code = document.createElement("pre");
    code.textContent = b.getAttribute("data-copy");
    b.replaceWith(code);
  }
  for (const b of main.querySelectorAll("button")) b.remove();
  const td = new TurndownService({ headingStyle: "atx", codeBlockStyle: "fenced", bulletListMarker: "-" });
  td.use(gfm);
  td.addRule("pre-as-shell", {
    filter: (node) => node.nodeName === "PRE" && !node.querySelector("code"),
    replacement: (content, node) => `\n\n\`\`\`sh\n${node.textContent.trim()}\n\`\`\`\n\n`,
  });
  return td
    .turndown(main.innerHTML)
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/** YAML frontmatter for a Markdown twin; values as JSON strings, which YAML reads as-is. */
export function withFrontmatter(meta, body) {
  const lines = Object.entries(meta).map(([k, v]) => `${k}: ${JSON.stringify(v)}`);
  return `---\n${lines.join("\n")}\n---\n\n${body.trim()}\n`;
}
