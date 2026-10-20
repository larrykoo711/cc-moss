// Renders the share cards (og/<page>.<lang>.png, 1200×630) with headless Chrome. The cards are
// committed, so the site build needs no browser; rerun this when a page's headline changes.
//   node scripts/gen-og.mjs
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { VERSION } from "../lib/site.mjs";

const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const loop = readFileSync("../docs/assets/logo.svg", "utf8").match(/<path d="([^"]+)"/)[1];

const CARDS = {
  "home.en": { eyebrow: "Session manager for Claude Code", title: "Every session.<br>Carried on.", sub: "See which sessions are busy, waiting or stopped. After a restart, bring them all back in one click." },
  "home.zh": { eyebrow: "Claude Code 会话管理", title: "文明会延续，<br>对话也是。", sub: "谁在忙、谁在等你、谁断了，一眼就知道。电脑重启后，一键全部找回。" },
  "changelog.en": { eyebrow: "Changelog", title: "What’s new,<br>version by version.", sub: "Every cc-moss release in plain words, newest first." },
  "changelog.zh": { eyebrow: "更新记录", title: "每一版，<br>都多照看一点。", sub: "cc-moss 的每一版都在这里，用大白话写清楚多了什么。" },
};

mkdirSync("og", { recursive: true });
for (const [name, card] of Object.entries(CARDS)) {
  const q = new URLSearchParams({ ...card, meta: `v${VERSION} · MIT`, loop });
  const page = `file://${resolve("scripts/og.html")}?${q}`;
  execFileSync(CHROME, ["--headless=new", "--hide-scrollbars", "--force-device-scale-factor=1", "--window-size=1200,630", "--virtual-time-budget=4000", `--screenshot=${resolve(`og/${name}.png`)}`, page], { stdio: "ignore" });
  console.log(`og/${name}.png`);
}
