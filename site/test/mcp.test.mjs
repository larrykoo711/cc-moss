import assert from "node:assert/strict";
import { test } from "node:test";
import { handleMessage, twinFor } from "../worker/mcp.mjs";
import { TOOLS } from "../worker/tools.mjs";

const deps = {
  search: async ({ query, locale, limit }) => [{ title: `hit for ${query}`, url: `/${locale}`, limit }],
  readDoc: async (path) => (twinFor(path) ? { path: twinFor(path).path, url: "https://x/", markdown: "# page" } : null),
  getRelease: async () => ({ version: "0.2.0" }),
};
const call = (method, params, id = 1) => handleMessage({ jsonrpc: "2.0", id, method, params }, deps);

test("initialize agrees on a protocol version it supports", async () => {
  assert.equal((await call("initialize", { protocolVersion: "2025-03-26" })).result.protocolVersion, "2025-03-26");
  assert.equal((await call("initialize", { protocolVersion: "1999-01-01" })).result.protocolVersion, "2025-06-18");
  assert.ok((await call("initialize", {})).result.instructions.includes("search_docs"));
});

test("notifications get no response", async () => {
  assert.equal(await handleMessage({ jsonrpc: "2.0", method: "notifications/initialized" }, deps), null);
});

test("batches and malformed requests are rejected", async () => {
  assert.equal((await handleMessage([], deps)).error.code, -32600);
  assert.equal((await handleMessage({ id: 1, method: "ping" }, deps)).error.code, -32600);
  assert.equal((await call("resources/list")).error.code, -32601);
});

test("tools/list returns the three tools, all read-only", async () => {
  const { tools } = (await call("tools/list")).result;
  assert.deepEqual(tools.map((t) => t.name), ["search_docs", "read_doc", "get_release"]);
  for (const t of tools) assert.equal(t.annotations.readOnlyHint, true);
  assert.equal(tools, TOOLS);
});

test("search_docs validates its input as a tool error, not a protocol error", async () => {
  const empty = (await call("tools/call", { name: "search_docs", arguments: {} })).result;
  assert.equal(empty.isError, true);
  const ok = (await call("tools/call", { name: "search_docs", arguments: { query: "restore", locale: "zh", limit: 50 } })).result;
  const body = JSON.parse(ok.content[0].text);
  assert.equal(body.locale, "zh");
  assert.equal(body.results[0].limit, 10);
});

test("an unknown tool is a protocol error", async () => {
  assert.equal((await call("tools/call", { name: "rm_rf", arguments: {} })).error.code, -32602);
});

test("read_doc returns Markdown, or a pointer to llms.txt when there is no such page", async () => {
  const hit = (await call("tools/call", { name: "read_doc", arguments: { path: "/guide" } })).result;
  assert.equal(hit.content[0].text, "# page");
  assert.equal(hit.structuredContent.path, "/guide/");
  const miss = (await call("tools/call", { name: "read_doc", arguments: { path: "/nope/" } })).result;
  assert.equal(miss.isError, true);
  assert.match(miss.content[0].text, /llms\.txt/);
});

test("get_release reports GitHub being unreachable as a tool error", async () => {
  const failing = { ...deps, getRelease: async () => { throw new Error("timeout"); } };
  const res = (await handleMessage({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name: "get_release" } }, failing)).result;
  assert.equal(res.isError, true);
});

test("twinFor accepts paths and URLs of the site's pages only", () => {
  assert.deepEqual(twinFor("/"), { path: "/", twin: "/index.md" });
  assert.deepEqual(twinFor("/zh"), { path: "/zh/", twin: "/zh/index.md" });
  assert.deepEqual(twinFor("https://cc-moss.larrykoo.com/guide/#install"), { path: "/guide/", twin: "/guide/index.md" });
  assert.deepEqual(twinFor("/zh/guide/index.md"), { path: "/zh/guide/", twin: "/zh/guide/index.md" });
  assert.deepEqual(twinFor("https://cc-moss.larrykoo.com/zh/changelog/#v0-3-0"), { path: "/zh/changelog/", twin: "/zh/changelog/index.md" });
  assert.equal(twinFor("/../etc/passwd"), null);
  assert.equal(twinFor("/assets/"), null);
});

import { readFileSync } from "node:fs";
import { releaseFromFeed } from "../worker/release.mjs";

test("get_release reads the newest entry of GitHub's releases feed", () => {
  const r = releaseFromFeed(readFileSync("test/fixtures/releases.atom", "utf8"));
  assert.match(r.version, /^\d+\.\d+\.\d+$/);
  assert.match(r.url, /^https:\/\/github\.com\/larrykoo711\/cc-moss\/releases\/tag\//);
  assert.ok(r.notes.length > 20 && !r.notes.includes("<"));
  assert.equal(r.update, "cc-moss update");
  assert.throws(() => releaseFromFeed("<feed></feed>"), /no release/);
});
