// Real requests against a running site: `node scripts/smoke.mjs [base]`, base defaulting to
// `wrangler dev` on :8788. The same checks run against production after a deploy.
const BASE = (process.argv[2] || "http://localhost:8788").replace(/\/$/, "");
let failed = 0;
const check = (ok, what, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || !detail ? "" : `: ${detail}`}`);
  if (!ok) failed++;
};
const get = (path, headers = {}) => fetch(BASE + path, { headers, redirect: "manual" });
const mcp = async (body) => {
  const res = await fetch(`${BASE}/mcp`, { method: "POST", headers: { "content-type": "application/json", accept: "application/json, text/event-stream" }, body: JSON.stringify(body) });
  return { status: res.status, json: res.status === 200 ? await res.json() : null };
};

for (const path of ["/", "/zh/"]) {
  const html = await get(path, { accept: "text/html" });
  check(html.status === 200 && html.headers.get("content-type").startsWith("text/html"), `${path} serves HTML`);
  check((html.headers.get("vary") || "").includes("Accept"), `${path} varies on Accept`);
  const link = html.headers.get("link") || "";
  check(link.includes('rel="alternate"; type="text/markdown"') && link.includes('rel="describedby"'), `${path} Link header`, link);
  const md = await get(path, { accept: "text/markdown" });
  const body = await md.text();
  check(md.status === 200 && md.headers.get("content-type").startsWith("text/markdown") && body.startsWith("---\ntitle:"), `${path} as Markdown`);
  const bot = await get(path, { "user-agent": "Mozilla/5.0 (compatible; ClaudeBot/1.0)", accept: "text/html" });
  check(bot.headers.get("content-type").startsWith("text/markdown"), `${path} as Markdown for an AI fetcher`);
  const crawler = await get(path, { "user-agent": "Mozilla/5.0 (compatible; Googlebot/2.1)", accept: "text/html,*/*" });
  check(crawler.headers.get("content-type").startsWith("text/html"), `${path} as HTML for a search crawler`);
}

for (const [path, home] of [["/guide/", "/#faq"], ["/zh/guide/", "/zh/#faq"]]) {
  const person = await get(path, { accept: "text/html" });
  check(person.status === 302 && person.headers.get("location") === home, `${path} sends people to the FAQ`);
  const agent = await get(path, { accept: "text/markdown" });
  check(agent.status === 200 && (await agent.text()).includes("cc-moss"), `${path} gives agents the full guide`);
  const md = await get(`${path}index.md`);
  check(md.status === 200 && md.headers.get("content-type").startsWith("text/markdown"), `${path}index.md`);
}
check((await get("/", { accept: "application/json" })).status === 406, "406 when neither HTML nor Markdown is acceptable");
const slashless = await get("/zh");
check(slashless.status === 301 && slashless.headers.get("location") === "/zh/", "/zh redirects to /zh/");
const missing = await get("/no-such-page", { accept: "text/html" });
check(missing.status === 404 && (await missing.text()).includes("Nothing here"), "404 page for people");
const missingMd = await get("/no-such-page", { accept: "text/markdown" });
check(missingMd.status === 404 && (await missingMd.text()).includes("llms.txt"), "404 in Markdown for agents");

for (const [path, type] of [
  ["/robots.txt", "text/plain"],
  ["/sitemap.xml", "application/xml"],
  ["/llms.txt", "text/plain"],
  ["/llms-full.txt", "text/plain"],
  ["/zh/llms.txt", "text/plain"],
  ["/index.md", "text/markdown"],
  ["/.well-known/mcp.json", "application/json"],
  ["/.well-known/mcp/server-card.json", "application/json"],
  ["/.well-known/agent-skills/index.json", "application/json"],
  ["/.well-known/agent-skills/cc-moss/SKILL.md", "text/markdown"],
  ["/.well-known/ai-catalog.json", "application/json"],
  ["/server.json", "application/json"],
]) {
  const res = await get(path);
  check(res.status === 200 && (res.headers.get("content-type") || "").startsWith(type), `${path} → 200 ${type}`, `${res.status} ${res.headers.get("content-type")}`);
}
check((await get("/.well-known/mcp.json")).headers.get("x-robots-tag") === "noindex", "machine files are noindex");

// MCP
const card = await get("/mcp");
check(card.status === 200 && (await card.json()).remotes?.[0]?.url?.endsWith("/mcp"), "GET /mcp returns the server card");
check((await get("/mcp", { accept: "text/event-stream" })).status === 405, "no SSE stream on GET /mcp");
const oversized = await fetch(`${BASE}/mcp`, { method: "POST", headers: { "content-type": "application/json" }, body: " ".repeat(70 * 1024) });
check(oversized.status === 413, "POST /mcp over 64 KB → 413", String(oversized.status));
const init = await mcp({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "smoke", version: "1" } } });
check(init.json?.result?.serverInfo?.name === "cc-moss-site", "MCP initialize");
check((await mcp({ jsonrpc: "2.0", method: "notifications/initialized" })).status === 202, "MCP notification → 202");
const list = await mcp({ jsonrpc: "2.0", id: 2, method: "tools/list" });
check(list.json?.result?.tools?.length === 3, "MCP tools/list → 3 tools");
const tool = async (name, args) => (await mcp({ jsonrpc: "2.0", id: 3, method: "tools/call", params: { name, arguments: args } })).json?.result;
const en = JSON.parse((await tool("search_docs", { query: "restore after reboot" })).content[0].text);
check(en.count > 0 && en.results[0].url.includes("cc-moss.larrykoo.com"), "search_docs finds 'restore after reboot'", JSON.stringify(en).slice(0, 200));
const zh = JSON.parse((await tool("search_docs", { query: "快捷键", locale: "zh" })).content[0].text);
check(zh.count > 0, "search_docs finds 快捷键 in Chinese");
const page = await tool("read_doc", { path: "/guide/" });
check(page?.content?.[0]?.text?.includes("# cc-moss guide"), "read_doc /guide/");
const faq = await tool("read_doc", { path: "/" });
check(faq?.content?.[0]?.text?.includes("Questions people ask"), "read_doc / includes the FAQ");
const release = await tool("get_release", {});
check(!release.isError && /^\d+\.\d+\.\d+/.test(JSON.parse(release.content[0].text).version), "get_release reads GitHub", release.content?.[0]?.text?.slice(0, 120));
const bad = await fetch(`${BASE}/mcp`, { method: "POST", headers: { "content-type": "application/json" }, body: "{nope" });
check(bad.status === 400, "MCP parse error → 400");

console.log(failed ? `\n${failed} failed` : "\nall passed");
process.exit(failed ? 1 : 0);
