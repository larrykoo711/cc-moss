// The guide lives in docs/ as Markdown for GitHub; the site hands the same text to agents.
// One source, so the site never says something the repo doesn't.
import { readFileSync } from "node:fs";
import { REPO } from "./site.mjs";

/**
 * The guide's Markdown as the site serves it: without the GitHub-only language bar, with links
 * that leave the docs folder pointing at GitHub.
 */
export function guideMarkdown(file) {
  const src = readFileSync(`../${file}`, "utf8");
  return src
    .replace(/^\*\*(English|简体中文)\*\* · .*\n\n/m, "")
    .replace(/\]\((?!https?:|#|mailto:)([^)]+)\)/g, (_, target) => `](${REPO}/blob/main/${resolveRepoPath(file, target)})`)
    // Pictures in docs/assets are served by the site itself.
    .replace(/<img src="assets\/([^"]+)"/g, '<img loading="lazy" src="/docs-assets/$1"');
}

/** The docs/assets files a guide shows, so the build can publish just those. */
export const guideImages = (markdown) => [...markdown.matchAll(/src="\/docs-assets\/([^"]+)"/g)].map((m) => m[1]);

function resolveRepoPath(from, target) {
  const parts = from.split("/").slice(0, -1);
  for (const seg of target.split("/")) {
    if (seg === "..") parts.pop();
    else if (seg !== ".") parts.push(seg);
  }
  return parts.join("/");
}
