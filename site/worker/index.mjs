// The site's Worker. Static files come from the ASSETS binding; this decides what each request
// gets: a page as HTML or Markdown, the MCP endpoint, the right headers on everything.
import { MAX_BODY, readCapped } from "./body.mjs";
import { handleMessage, twinFor } from "./mcp.mjs";
import { acceptsNeither, wantsMarkdown } from "./negotiate.mjs";
import { search } from "./search.mjs";
import { releaseFromFeed } from "./release.mjs";
import { serverCard } from "./tools.mjs";

const PAGES = new Set(["/", "/zh/", "/changelog/", "/zh/changelog/"]);
// The full guide is for agents: Markdown to them, and people are sent to the FAQ instead.
const AGENT_ONLY = new Map([["/guide/", "/#faq"], ["/zh/guide/", "/zh/#faq"]]);

const SECURITY = {
  "x-content-type-options": "nosniff",
  "referrer-policy": "strict-origin-when-cross-origin",
  "permissions-policy": "camera=(), microphone=(), geolocation=()",
  "strict-transport-security": "max-age=31536000",
};

const TYPES = {
  ".md": "text/markdown; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".xml": "application/xml; charset=utf-8",
  ".json": "application/json; charset=utf-8",
};

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const path = url.pathname;

    if (path === "/mcp" || path === "/mcp/") return serveMcp(request, env, ctx);
    if (request.method === "OPTIONS") return respond(null, 204, { allow: "GET, HEAD, OPTIONS" });
    if (request.method !== "GET" && request.method !== "HEAD") return respond("Method not allowed\n", 405, { allow: "GET, HEAD, OPTIONS" });

    // One URL per page: /zh → /zh/, /guide → /guide/, /index.html → /.
    if (PAGES.has(`${path}/`) || AGENT_ONLY.has(`${path}/`)) return redirect(url, `${path}/`);
    if (AGENT_ONLY.has(path)) {
      if (wantsMarkdown(request)) return markdown(await asset(env, `${path}index.md`), 200);
      return respond(null, 302, { location: AGENT_ONLY.get(path), vary: "Accept, User-Agent" });
    }
    if (path.endsWith("/index.html") && PAGES.has(path.slice(0, -10))) return redirect(url, path.slice(0, -10));
    if (PAGES.has(path)) return servePage(request, env, path);

    const res = await env.ASSETS.fetch(request);
    if (res.status === 404) return notFound(request, env);
    return dress(res, path);
  },
};

// ---- pages ----------------------------------------------------------------------------

async function servePage(request, env, path) {
  const accept = request.headers.get("accept");
  if (acceptsNeither(accept) && !wantsMarkdown(request))
    return respond("This page is available as text/html or text/markdown.\n", 406, { vary: "Accept, User-Agent" });
  const twin = `${path}index.md`;
  if (wantsMarkdown(request)) {
    const md = await asset(env, twin);
    return markdown(md, 200, path);
  }
  const res = await env.ASSETS.fetch(new Request(new URL(path, request.url), request));
  const headers = new Headers(res.headers);
  headers.set("vary", "Accept, User-Agent");
  headers.set(
    "link",
    [
      `<${twin}>; rel="alternate"; type="text/markdown"`,
      `</llms.txt>; rel="describedby"; type="text/plain"`,
      `<${path.startsWith("/zh/") ? "/zh/guide/index.md" : "/guide/index.md"}>; rel="service-doc"; type="text/markdown"`,
      `</.well-known/mcp/server-card.json>; rel="mcp-server-card"; type="application/json"`,
    ].join(", "),
  );
  headers.set("cache-control", "public, max-age=0, must-revalidate");
  for (const [k, v] of Object.entries(SECURITY)) headers.set(k, v);
  return new Response(res.body, { status: res.status, headers });
}

function markdown(text, status, canonicalPath) {
  const headers = {
    "content-type": TYPES[".md"],
    vary: "Accept, User-Agent",
    "x-markdown-tokens": String(Math.ceil(text.length / 4)),
    "cache-control": "public, max-age=300",
  };
  if (canonicalPath) headers.link = `<${canonicalPath}>; rel="canonical"; type="text/html"`;
  return respond(text, status, headers);
}

async function notFound(request, env) {
  const path = new URL(request.url).pathname;
  if (wantsMarkdown(request) || path.endsWith(".md")) return markdown(await asset(env, "/404.md"), 404);
  const res = await env.ASSETS.fetch(new Request(new URL("/404.html", request.url)));
  return respond(await res.text(), 404, { "content-type": "text/html; charset=utf-8", vary: "Accept, User-Agent" });
}

/** Content types the asset server can't guess, and noindex on files meant for machines. */
function dress(res, path) {
  const headers = new Headers(res.headers);
  const ext = path.match(/\.[a-z]+$/)?.[0];
  if (TYPES[ext]) headers.set("content-type", TYPES[ext]);
  if (path === "/.well-known/api-catalog") headers.set("content-type", "application/linkset+json");
  if (path.startsWith("/.well-known/") || path === "/search-records.json" || path === "/server.json")
    headers.set("x-robots-tag", "noindex");
  if (path.startsWith("/.well-known/") || path === "/server.json" || path === "/llms.txt" || path === "/llms-full.txt")
    headers.set("access-control-allow-origin", "*");
  if (ext === ".md") headers.set("vary", "Accept, User-Agent");
  for (const [k, v] of Object.entries(SECURITY)) headers.set(k, v);
  return new Response(res.body, { status: res.status, headers });
}

// ---- MCP ------------------------------------------------------------------------------

const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "POST, GET, OPTIONS",
  "access-control-allow-headers": "content-type, mcp-protocol-version, mcp-session-id, authorization",
};

async function serveMcp(request, env, ctx) {
  const json = (body, status = 200) => respond(JSON.stringify(body), status, { ...CORS, "content-type": "application/json", "x-robots-tag": "noindex" });
  if (request.method === "OPTIONS") return respond(null, 204, CORS);
  if (request.method === "GET" || request.method === "HEAD") {
    // No server-to-client stream here: this server only answers requests.
    if ((request.headers.get("accept") || "").includes("text/event-stream")) return respond(null, 405, { ...CORS, allow: "POST" });
    return json(serverCard());
  }
  if (request.method !== "POST") return respond(null, 405, { ...CORS, allow: "POST, GET, OPTIONS" });

  const body = await readCapped(request, MAX_BODY);
  if (body === null) return json({ jsonrpc: "2.0", id: null, error: { code: -32600, message: "Request too large" } }, 413);
  let msg;
  try {
    msg = JSON.parse(body);
  } catch {
    return json({ jsonrpc: "2.0", id: null, error: { code: -32700, message: "Parse error" } }, 400);
  }
  const result = await handleMessage(msg, deps(request, env, ctx));
  if (result === null) return respond(null, 202, CORS);
  return json(result);
}

let records = null;

function deps(request, env, ctx) {
  return {
    async search(q) {
      if (!records) records = JSON.parse(await asset(env, "/search-records.json"));
      return search(records, q);
    },
    async readDoc(input) {
      const page = twinFor(input);
      if (!page) return null;
      const md = await asset(env, page.twin, true);
      return md === null ? null : { path: page.path, url: new URL(page.path, request.url).href, markdown: md };
    },
    getRelease: () => latestRelease(ctx),
  };
}

// GitHub's REST API rate-limits Cloudflare's shared addresses, so read the releases feed instead.
async function latestRelease(ctx) {
  const feed = "https://github.com/larrykoo711/cc-moss/releases.atom";
  const cache = caches.default;
  const key = new Request(feed);
  let res = await cache.match(key);
  if (!res) {
    const live = await fetch(feed, { headers: { "user-agent": "cc-moss-site" } });
    if (!live.ok) throw new Error(`GitHub answered ${live.status}`);
    res = new Response(await live.text(), { headers: { "content-type": "application/atom+xml", "cache-control": "public, max-age=3600" } });
    ctx.waitUntil(cache.put(key, res.clone()));
  }
  return releaseFromFeed(await res.text());
}

// ---- helpers --------------------------------------------------------------------------

async function asset(env, path, nullable = false) {
  const res = await env.ASSETS.fetch(new Request(`https://assets.local${path}`));
  if (!res.ok) {
    if (nullable) return null;
    throw new Error(`missing asset ${path}`);
  }
  return res.text();
}

function redirect(url, to) {
  return respond(null, 301, { location: to + url.search });
}

function respond(body, status, headers = {}) {
  const h = new Headers(headers);
  for (const [k, v] of Object.entries(SECURITY)) if (!h.has(k)) h.set(k, v);
  return new Response(body, { status, headers: h });
}
