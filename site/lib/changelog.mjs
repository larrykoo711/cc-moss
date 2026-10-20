// The Changelog page for people: a timeline built from src/changelog.json (plain words, both
// languages). CHANGELOG.md keeps the technical notes; the test makes sure no release is missing here.
import { readFileSync } from "node:fs";

export const loadChangelog = (file = "src/changelog.json") => JSON.parse(readFileSync(file, "utf8")).versions;

/** `## [0.3.0] - 2026-10-19` headings in CHANGELOG.md, newest first; `[Unreleased]` has no date. */
export function changelogVersions(markdown) {
  return [...markdown.matchAll(/^## \[([^\]]+)\](?: - (\d{4}-\d{2}-\d{2}))?/gm)].map((m) => ({ version: m[1], date: m[2] || null }));
}

/** `0.3.1` → `v0-3-1`, the anchor of its card. */
export const anchorOf = (version) => `v${version.replaceAll(".", "-")}`;

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const both = (zh, en, tag = "span") => `<${tag} lang="zh">${zh}</${tag}><${tag} lang="en">${en}</${tag}>`;

function dateText(iso) {
  const [y, m, d] = iso.split("-").map(Number);
  return both(`${y} 年 ${m} 月 ${d} 日`, `${MONTHS[m - 1]} ${d}, ${y}`);
}

// One small drawing per release, stroked like the site's other icons.
const ICONS = {
  restore: '<path d="M4 12a8 8 0 1 0 2.4-5.7"/><path d="M4 4v4h4"/><path d="M12 8v4l3 2"/>',
  picker: '<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M7 9h2M11 9h6M7 13h2M11 13h4M7 17h2"/>',
  menubar: '<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M3 8.5h18"/><path d="M15 6.25h3"/><rect x="11" y="11" width="7" height="6" rx="1.5"/>',
  tabs: '<path d="M3 9V6.5A2.5 2.5 0 0 1 5.5 4h4l2 3H18.5A2.5 2.5 0 0 1 21 9.5v8A2.5 2.5 0 0 1 18.5 20h-13A2.5 2.5 0 0 1 3 17.5z"/><path d="M3 9h18"/>',
  rows: '<rect x="3" y="5" width="18" height="6" rx="2"/><rect x="3" y="13" width="18" height="6" rx="2"/><path d="M15 8h3M15 16h3"/>',
  moss: '<path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v7a2.5 2.5 0 0 1-2.5 2.5H11l-4 4v-4h-.5A2.5 2.5 0 0 1 4 13.5z"/><path d="M8.5 10c1-1.4 2-1.4 3.5 0s2.5 1.4 3.5 0"/>',
  archive: '<rect x="3" y="4" width="18" height="5" rx="1.5"/><path d="M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9"/><path d="M10 13h4"/>',
};

const icon = (name, cls) =>
  `<svg viewBox="0 0 24 24" class="${cls}" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name] || ICONS.restore}</svg>`;

/** The version list beside the timeline (a jump list; agents skip it). */
export function changelogIndex(versions) {
  const items = versions
    .map(
      (v, i) =>
        `<li><a href="#${anchorOf(v.version)}" data-v="${anchorOf(v.version)}" class="cl-jump flex items-center gap-3 rounded-full px-3 py-1.5 font-mono text-[13px] text-fg/50 hover:text-fg hover:bg-fg/[.05] transition-colors"><span class="w-1.5 h-1.5 rounded-full ${i === 0 ? "bg-coral" : "bg-fg/25"}"></span>${esc(v.version)}</a></li>`,
    )
    .join("\n          ");
  return items;
}

/** Every release as a card on a vertical line, newest first. */
export function changelogTimeline(versions) {
  return versions
    .map((v, i) => {
      const id = anchorOf(v.version);
      const latest = i === 0;
      const highlights = v.highlights
        .map(
          (h) =>
            `<li class="flex gap-3"><svg class="mt-[0.45em] w-3.5 h-2 shrink-0 ${latest ? "text-coral" : "text-coral/70"}" aria-hidden="true"><use href="#continuum"/></svg><span>${both(esc(h.zh), esc(h.en))}</span></li>`,
        )
        .join("\n              ");
      return `<li data-agent="unwrap" class="release reveal relative pl-10 sm:pl-16 pb-10 sm:pb-14 last:pb-0" style="--d:${Math.min(i, 3) * 60}ms">
          <span class="cl-node ${latest ? "is-latest" : ""} absolute left-0 sm:left-3 top-7 grid place-items-center w-[1.625rem] h-[1.625rem] rounded-full" aria-hidden="true">${latest ? '<svg class="w-4 h-2.5 text-white"><use href="#continuum"/></svg>' : '<span class="w-2 h-2 rounded-full bg-fg/40"></span>'}</span>
          <article class="shell ${latest ? "cl-latest" : ""}" aria-labelledby="${id}">
            <div class="core relative overflow-hidden p-6 sm:p-9">
              ${icon(v.icon, `hidden sm:block absolute right-6 top-6 w-24 h-24 ${latest ? "text-coral/[.13]" : "text-fg/[.06]"}`)}
              <div class="relative flex flex-col">
                <h2 id="${id}" class="mt-4 max-w-[34rem] text-[1.5rem] sm:text-[1.875rem] font-semibold tracking-[-0.03em] leading-[1.15] scroll-mt-52"><span class="visually-hidden">${esc(v.version)} · </span>${both(esc(v.headline.zh), esc(v.headline.en))}</h2>
                <p class="order-first flex flex-wrap items-center gap-x-3 gap-y-2 text-sm">
                  <span class="rounded-full px-2.5 py-0.5 font-mono text-[13px] font-medium ${latest ? "bg-coral text-white" : "bg-fg/[.07] text-fg/80"}" aria-hidden="true">${esc(v.version)}</span>
                  <time datetime="${v.date}" class="text-fg/45">${dateText(v.date)}</time>
                  ${latest ? `<span class="rounded-full bg-coral/15 text-coral px-2 py-0.5 text-xs font-medium">${both("最新", "Latest")}</span>` : ""}
                </p>
              </div>
              <ul class="relative mt-5 space-y-2.5 text-fg/65 leading-relaxed max-w-[38rem]">
              ${highlights}
              </ul>
            </div>
          </article>
        </li>`;
    })
    .join("\n        ");
}
