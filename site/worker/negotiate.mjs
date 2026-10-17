// Who gets Markdown instead of HTML. A client that explicitly prefers text/markdown (q at least
// HTML's) gets it; so do AI assistants' fetchers. Search engine crawlers never do: they index the
// HTML page that people see.

/** Parse an Accept header into [{type, q}]. */
function parseAccept(header) {
  return String(header || "")
    .split(",")
    .map((part) => {
      const [type, ...params] = part.trim().toLowerCase().split(";");
      const q = params.map((p) => p.trim()).find((p) => p.startsWith("q="));
      return { type: type.trim(), q: q ? Number(q.slice(2)) : 1 };
    })
    .filter((x) => x.type && !Number.isNaN(x.q));
}

const qFor = (list, type) => {
  const exact = list.find((x) => x.type === type);
  if (exact) return exact.q;
  const [major] = type.split("/");
  const group = list.find((x) => x.type === `${major}/*`);
  if (group) return group.q;
  const any = list.find((x) => x.type === "*/*");
  return any ? any.q : 0;
};

/** True when the client names text/markdown and rates it at least as high as HTML. */
export function prefersMarkdown(accept) {
  const list = parseAccept(accept);
  const md = list.find((x) => x.type === "text/markdown");
  if (!md || md.q <= 0) return false;
  return md.q >= qFor(list, "text/html");
}

/** True when the client takes neither HTML nor Markdown (so the page can't be served: 406). */
export function acceptsNeither(accept) {
  if (!accept) return false;
  const list = parseAccept(accept);
  return qFor(list, "text/html") <= 0 && qFor(list, "text/markdown") <= 0;
}

// AI assistants' fetchers and retrieval bots. Not Googlebot or Bingbot: search indexes the HTML.
export const AI_AGENTS = [
  "ChatGPT-User", "GPTBot", "OAI-SearchBot", "ClaudeBot", "Claude-User", "Claude-SearchBot", "anthropic-ai",
  "PerplexityBot", "Perplexity-User", "Google-Extended", "Applebot-Extended", "meta-externalagent",
  "MistralAI-User", "cohere-ai", "DuckAssistBot", "YouBot", "Amazonbot",
];

const AGENT_RE = new RegExp(`(^|[\\s;(/])(${AI_AGENTS.map((a) => a.replace(/[-]/g, "\\-")).join("|")})\\b`, "i");

export function isAiAgent(userAgent) {
  return AGENT_RE.test(String(userAgent || ""));
}

/** Markdown or HTML for a page request. */
export function wantsMarkdown(request) {
  const accept = request.headers.get("accept");
  return prefersMarkdown(accept) || isAiAgent(request.headers.get("user-agent"));
}
