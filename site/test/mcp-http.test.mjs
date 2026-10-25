import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_BODY } from "../worker/body.mjs";
import * as entry from "../worker/index.mjs";

const worker = entry.default;

const URL_MCP = "https://cc-moss.example/mcp";

/** A body stream of `chunks` pieces of `size` bytes; `pulled` counts what the Worker actually read. */
function stream(chunks, size) {
  const seen = { pulled: 0 };
  let left = chunks;
  const body = new ReadableStream({
    pull(controller) {
      if (left-- <= 0) return controller.close();
      seen.pulled += 1;
      controller.enqueue(new Uint8Array(size).fill(0x20));
    },
  }, { highWaterMark: 0 }); // pull only when read, so `pulled` is what the Worker asked for
  return { body, seen };
}

const post = (body, headers = {}) =>
  worker.fetch(new Request(URL_MCP, { method: "POST", body, headers, duplex: "half" }), {}, {});

test("the entry module exports only its handler, as the Workers runtime requires", () => {
  assert.deepEqual(Object.keys(entry), ["default"]);
});

test("the body limit is small enough for a public endpoint", () => {
  assert.ok(MAX_BODY <= 64 * 1024, `${MAX_BODY}`);
});

test("a declared length over the limit is refused without reading the body", async () => {
  const { body, seen } = stream(4, 1024);
  const res = await post(body, { "content-length": String(MAX_BODY + 1) });
  assert.equal(res.status, 413);
  assert.equal(seen.pulled, 0);
});

test("an undeclared body is cut off once it passes the limit", async () => {
  const chunk = 16 * 1024;
  const total = 1000;
  const { body, seen } = stream(total, chunk);
  const res = await post(body);
  assert.equal(res.status, 413);
  assert.ok(seen.pulled <= Math.ceil(MAX_BODY / chunk) + 2, `read ${seen.pulled} of ${total} chunks`);
});

test("the limit counts bytes, not characters", async () => {
  // Three bytes per character in UTF-8: under the limit in characters, over it in bytes.
  const text = JSON.stringify({ jsonrpc: "2.0", id: 1, method: "ping", params: { q: "汉".repeat(MAX_BODY / 2) } });
  assert.ok(text.length < MAX_BODY * 1.1 && new TextEncoder().encode(text).length > MAX_BODY);
  assert.equal((await post(text)).status, 413);
});

test("a normal request still gets its answer", async () => {
  const res = await post(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "ping" }), {
    "content-type": "application/json",
  });
  assert.equal(res.status, 200);
  assert.deepEqual(await res.json(), { jsonrpc: "2.0", id: 1, result: {} });
});

test("a body that is not JSON is still a parse error, not a size error", async () => {
  assert.equal((await post("{")).status, 400);
});
