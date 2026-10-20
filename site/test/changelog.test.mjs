// The Changelog page is hand-written for people, so a release can be forgotten. These checks make
// sure every version in CHANGELOG.md has its card, with the same date, written in both languages.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { anchorOf, changelogVersions, loadChangelog } from "../lib/changelog.mjs";
import { VERSION } from "../lib/site.mjs";

const entries = loadChangelog();
const released = changelogVersions(readFileSync("../CHANGELOG.md", "utf8"));
const semver = (v) => v.split(".").map(Number);
const newer = (a, b) => {
  const [x, y] = [semver(a), semver(b)];
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i];
  return false;
};

test("every version in CHANGELOG.md has an entry, with the same date", () => {
  for (const { version, date } of released.filter((r) => r.version !== "Unreleased")) {
    const entry = entries.find((e) => e.version === version);
    assert.ok(entry, `site/src/changelog.json has no entry for ${version}`);
    assert.equal(entry.date, date, `${version}: the date differs from CHANGELOG.md`);
  }
});

test("only the next release may be listed before CHANGELOG.md dates it", () => {
  const known = new Set(released.map((r) => r.version));
  const ahead = entries.filter((e) => !known.has(e.version));
  assert.ok(ahead.length <= 1, `not in CHANGELOG.md: ${ahead.map((e) => e.version).join(", ")}`);
  if (ahead.length) {
    assert.equal(ahead[0], entries[0], `${ahead[0].version} isn't in CHANGELOG.md and isn't the newest entry`);
    assert.ok(known.has("Unreleased"), `${ahead[0].version} is listed but CHANGELOG.md has no [Unreleased] section`);
  }
});

test("the version being built has an entry", () => {
  assert.ok(entries.some((e) => e.version === VERSION), `Cargo.toml says ${VERSION}`);
});

test("newest first, one entry per version, dates never go back in time", () => {
  assert.equal(new Set(entries.map((e) => e.version)).size, entries.length);
  for (let i = 1; i < entries.length; i++) {
    assert.ok(newer(entries[i - 1].version, entries[i].version), `${entries[i - 1].version} before ${entries[i].version}`);
    assert.ok(entries[i - 1].date >= entries[i].date, `${entries[i - 1].version} is dated before ${entries[i].version}`);
  }
});

test("each entry has a date, a headline and 3 to 5 highlights, in Chinese and in English", () => {
  for (const e of entries) {
    assert.match(e.date, /^\d{4}-\d{2}-\d{2}$/, e.version);
    assert.match(anchorOf(e.version), /^v\d+-\d+-\d+$/, e.version);
    for (const lang of ["zh", "en"]) {
      assert.ok(e.headline[lang]?.trim(), `${e.version} headline.${lang}`);
      for (const h of e.highlights) assert.ok(h[lang]?.trim(), `${e.version} highlight.${lang}`);
    }
    assert.ok(e.highlights.length >= 3 && e.highlights.length <= 5, `${e.version}: ${e.highlights.length} highlights`);
  }
  // The home page's badge names the newest release.
  assert.ok(entries[0].short?.zh && entries[0].short?.en, "the newest entry needs a short line for the badge");
});
