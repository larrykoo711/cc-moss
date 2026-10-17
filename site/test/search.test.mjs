import assert from "node:assert/strict";
import { test } from "node:test";
import { search, tokenize } from "../worker/search.mjs";

test("Chinese is split into overlapping pairs, English into words and their parts", () => {
  assert.deepEqual(tokenize("快捷键"), ["快捷", "捷键"]);
  assert.deepEqual(tokenize("cc-moss restore"), ["cc-moss", "cc", "moss", "restore"]);
  assert.deepEqual(tokenize("how do I restore"), ["restore"]);
});

const records = [
  { locale: "en", title: "Guide", heading: "Restore after a reboot", content: "Run cc-moss restore to bring back every interrupted session.", url: "u1" },
  { locale: "en", title: "Guide", heading: "Install", content: "brew install larrykoo711/tap/cc-moss, then restore works.", url: "u2" },
  { locale: "zh", title: "使用手册", heading: "唤出快捷键", content: "默认连按两下 Control 唤出面板。", url: "u3" },
];

test("headings outrank body text, and every word must match", () => {
  const hits = search(records, { query: "restore", locale: "en" });
  assert.deepEqual(hits.map((h) => h.url), ["u1", "u2"]);
  assert.deepEqual(search(records, { query: "restore homebrew", locale: "en" }), []);
});

test("Chinese queries find Chinese sections only", () => {
  assert.deepEqual(search(records, { query: "快捷键", locale: "zh" }).map((h) => h.url), ["u3"]);
  assert.deepEqual(search(records, { query: "快捷键", locale: "en" }), []);
});

test("prefixes match longer words", () => {
  assert.deepEqual(search(records, { query: "interrupt", locale: "en" }).map((h) => h.url), ["u1"]);
});
