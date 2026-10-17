// Builds the landing page into dist/: Tailwind compiled and minified, then
// the page, its script and its assets copied next to it.
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";

rmSync("dist", { recursive: true, force: true });
mkdirSync("dist");
execFileSync("node_modules/.bin/tailwindcss", ["-i", "src/styles.css", "-o", "dist/styles.css", "--minify"], { stdio: "inherit" });
// The page's ∞ is the brand logo's own path, read from the repo's logo.
const continuum = readFileSync("../docs/assets/logo.svg", "utf8").match(/<path d="([^"]+)"/)[1];
writeFileSync("dist/index.html", readFileSync("src/index.template.html", "utf8").replace("__CONTINUUM__", continuum));
for (const f of ["app.js", "favicon.svg"]) cpSync(`src/${f}`, `dist/${f}`);
cpSync("assets", "dist/assets", { recursive: true });
console.log("built dist/");
