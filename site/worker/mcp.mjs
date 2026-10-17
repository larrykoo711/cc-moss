// MCP over stateless Streamable HTTP: one JSON-RPC request in, one JSON response out, no
// sessions, no SSE. Read-only and anonymous. Dependencies are passed in so this runs in tests.
import { INSTRUCTIONS, PROTOCOL_VERSIONS, SERVER_INFO, TOOLS } from "./tools.mjs";

const reply = (id, result) => ({ jsonrpc: "2.0", id, result });
const failure = (id, code, message) => ({ jsonrpc: "2.0", id: id ?? null, error: { code, message } });
const text = (value) => ({ content: [{ type: "text", text: typeof value === "string" ? value : JSON.stringify(value, null, 2) }] });
const toolError = (message) => ({ content: [{ type: "text", text: message }], isError: true });

/**
 * Handle one parsed JSON-RPC message. Returns the response object, or null for a notification
 * (the HTTP layer answers those with 202 and no body).
 *
 * deps: { search({query, locale, limit}), readDoc(path) → {path, url, markdown} | null, getRelease() }
 */
export async function handleMessage(msg, deps) {
  if (Array.isArray(msg)) return failure(null, -32600, "Batch requests are not supported");
  if (!msg || msg.jsonrpc !== "2.0" || typeof msg.method !== "string") return failure(msg?.id, -32600, "Invalid request");
  const { id, method, params = {} } = msg;
  if (id === undefined || method.startsWith("notifications/")) return null;

  switch (method) {
    case "initialize": {
      const asked = params.protocolVersion;
      return reply(id, {
        protocolVersion: PROTOCOL_VERSIONS.includes(asked) ? asked : PROTOCOL_VERSIONS[0],
        capabilities: { tools: { listChanged: false } },
        serverInfo: SERVER_INFO,
        instructions: INSTRUCTIONS,
      });
    }
    case "ping":
      return reply(id, {});
    case "tools/list":
      return reply(id, { tools: TOOLS });
    case "tools/call":
      if (!TOOLS.some((t) => t.name === params.name)) return failure(id, -32602, `Unknown tool: ${params.name}`);
      return reply(id, await callTool(params.name, params.arguments || {}, deps));
    default:
      return failure(id, -32601, `Method not found: ${method}`);
  }
}

async function callTool(name, args, deps) {
  if (name === "search_docs") {
    const query = typeof args.query === "string" ? args.query.trim() : "";
    if (!query) return toolError("query is required: what to look for, e.g. 'restore after reboot'.");
    if (query.length > 200) return toolError("query is too long; keep it under 200 characters.");
    const locale = args.locale === "zh" ? "zh" : "en";
    const limit = Number.isInteger(args.limit) ? Math.min(10, Math.max(1, args.limit)) : 5;
    const results = await deps.search({ query, locale, limit });
    return text({ query, locale, count: results.length, results });
  }
  if (name === "read_doc") {
    if (typeof args.path !== "string" || !args.path.trim()) return toolError("path is required, e.g. /guide/ or /zh/.");
    const page = await deps.readDoc(args.path);
    if (!page) return toolError(`No page at ${args.path}. Pages are listed at https://cc-moss.larrykoo.com/llms.txt.`);
    return { content: [{ type: "text", text: page.markdown }], structuredContent: { path: page.path, url: page.url } };
  }
  if (name === "get_release") {
    try {
      return text(await deps.getRelease());
    } catch (e) {
      return toolError(`Could not reach GitHub for the latest release (${e.message}). See https://github.com/larrykoo711/cc-moss/releases/latest.`);
    }
  }
  return toolError(`Unknown tool: ${name}`);
}

/** Which page a path or URL means, as its Markdown twin. Only the site's own pages. */
export function twinFor(input) {
  let path = String(input).trim();
  try {
    if (/^https?:/i.test(path)) path = new URL(path).pathname;
  } catch {
    return null;
  }
  path = path.replace(/[?#].*$/, "").replace(/\/index\.md$/, "/").replace(/\.md$/, "/");
  if (!path.startsWith("/")) path = `/${path}`;
  if (!path.endsWith("/")) path += "/";
  if (path.includes("..")) return null;
  const pages = ["/", "/zh/", "/guide/", "/zh/guide/"];
  return pages.includes(path) ? { path, twin: `${path}index.md` } : null;
}
