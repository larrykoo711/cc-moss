// The site's facts, in one place: where it lives, its languages, its pages.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

export const ORIGIN = "https://cc-moss.larrykoo.com";
export const REPO = "https://github.com/larrykoo711/cc-moss";
export const LOCALES = ["en", "zh"];
export const DEFAULT_LOCALE = "en";
export const HTML_LANG = { en: "en", zh: "zh-CN" };

/** The workspace version, so the site never states a version the code doesn't have. */
export const VERSION = readFileSync("../Cargo.toml", "utf8").match(/\[workspace\.package\][^[]*?version\s*=\s*"([^"]+)"/)[1];

/** `/` for English, `/zh/` for Chinese, then the page's own path. */
export function localePath(locale, path = "/") {
  return (locale === DEFAULT_LOCALE ? "" : `/${locale}`) + path;
}

export const url = (path) => ORIGIN + path;

/** Every page the site publishes: `path` is the same in each language. */
export const PAGES = [
  {
    id: "home",
    path: "/",
    template: "src/index.template.html",
    sources: ["src/index.template.html", "src/partials/nav.html", "src/app.js", "src/changelog.json"],
    title: {
      en: "cc-moss: recover Claude Code sessions after a restart",
      zh: "cc-moss · Claude Code 会话管理：重启后一键找回全部会话",
    },
    description: {
      en: "Track every Claude Code session — busy, waiting or stopped — answer the ones waiting on you, and bring them all back after a restart, in one click. Free and open source, for macOS and Linux.",
      zh: "cc-moss 照看你的每个 Claude Code 会话：谁在忙、谁在等你、谁断了，一眼就知道；在等你的点一下就答，电脑重启后一键全部找回。免费开源，支持 macOS 与 Linux。",
    },
  },
  {
    id: "changelog",
    path: "/changelog/",
    template: "src/changelog.template.html",
    sources: ["src/changelog.template.html", "src/changelog.json", "lib/changelog.mjs", "src/partials/nav.html"],
    title: {
      en: "What's new in cc-moss · Changelog",
      zh: "cc-moss 更新记录：每一版新增了什么",
    },
    description: {
      en: "Every cc-moss release in plain words, newest first: what each version lets you do, from the first release to today.",
      zh: "cc-moss 的每一版都在这里，最新的在最上面：用大白话写清楚每一版让你多做到了什么。",
    },
  },
];

/**
 * Read by agents, not shown to people: the full guide, as Markdown only (no HTML page, not in
 * the sitemap). People get the FAQ on the home page; agents get every detail.
 */
export const AGENT_DOCS = [
  {
    id: "guide",
    path: "/guide/",
    source: { en: "docs/guide.en.md", zh: "docs/guide.md" },
    sources: [],
    title: {
      en: "cc-moss guide — install, commands, menu bar app and FAQ",
      zh: "cc-moss 使用手册：安装、命令、状态栏 App 与常见问题",
    },
    description: {
      en: "Everything about cc-moss: installing and updating, the t picker and its commands, the macOS menu bar app, Mo and notifications, configuration, privacy and FAQ.",
      zh: "cc-moss 的完整使用手册：安装与更新、t 选择器和全部命令、macOS 状态栏 App、小莫与通知、配置、隐私和常见问题。",
    },
  },
];

/**
 * The date a set of files last changed, from git; never the build date, which would move on
 * every deploy and teach search engines to ignore it. Uncommitted edits count as today.
 */
export function lastModified(files) {
  const paths = files.map((f) => (f.startsWith("docs/") ? `../${f}` : f));
  let date = "";
  try {
    date = execFileSync("git", ["log", "-1", "--format=%cs", "--", ...paths], { encoding: "utf8" }).trim();
    const dirty = execFileSync("git", ["status", "--porcelain", "--", ...paths], { encoding: "utf8" }).trim();
    if (dirty) date = new Date().toISOString().slice(0, 10);
  } catch {}
  return date || new Date().toISOString().slice(0, 10);
}
