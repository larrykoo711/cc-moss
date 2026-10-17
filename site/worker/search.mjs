// Search over the site's sections. Small enough to run per request: a few hundred records.
// English is split into words; Chinese, which has no spaces, into overlapping pairs of characters.

const CJK = /[㐀-鿿豈-﫿]/;
const STOP = new Set(["the", "a", "an", "to", "of", "and", "or", "in", "on", "for", "is", "it", "how", "do", "i", "my", "with", "what", "的", "了", "吗", "呢", "怎么", "什么"]);

export function tokenize(text) {
  const out = [];
  for (const run of String(text).toLowerCase().match(/[㐀-鿿豈-﫿]+|[a-z0-9_.\-]+/g) || []) {
    if (CJK.test(run)) {
      if (run.length === 1) out.push(run);
      for (let i = 0; i + 1 < run.length; i++) out.push(run.slice(i, i + 2));
    } else {
      const word = run.replace(/^[.\-]+|[.\-]+$/g, "");
      if (word && !STOP.has(word)) {
        out.push(word);
        // `cc-moss` and `claude --resume` are also found by their parts.
        for (const part of word.split(/[.\-_]+/)) if (part && part !== word && !STOP.has(part)) out.push(part);
      }
    }
  }
  return out.filter((t) => !STOP.has(t));
}

const WEIGHT = { title: 6, heading: 4, text: 1 };
const indexed = new WeakMap();

function prepare(records) {
  let prepared = indexed.get(records);
  if (!prepared) {
    prepared = records.map((r) => ({
      r,
      heading: new Set(tokenize(`${r.title} ${r.heading || ""}`)),
      text: new Set(tokenize(r.content)),
    }));
    indexed.set(records, prepared);
  }
  return prepared;
}

const hit = (set, token) => {
  if (set.has(token)) return true;
  if (CJK.test(token) || token.length < 3) return false;
  for (const t of set) if (t.startsWith(token)) return true;
  return false;
};

/**
 * Every query token must appear in the section (heading or text). Headings weigh more than body
 * text. Returns at most `limit` sections, best first, each with a short excerpt.
 */
export function search(records, { query, locale = "en", limit = 5 }) {
  const tokens = [...new Set(tokenize(query))];
  if (!tokens.length) return [];
  const results = [];
  for (const p of prepare(records)) {
    if (p.r.locale !== locale) continue;
    let score = 0;
    let all = true;
    for (const t of tokens) {
      const inHeading = hit(p.heading, t);
      const inText = hit(p.text, t);
      if (!inHeading && !inText) { all = false; break; }
      score += (inHeading ? WEIGHT.heading : 0) + (inText ? WEIGHT.text : 0);
    }
    if (all) results.push({ score, r: p.r });
  }
  results.sort((a, b) => b.score - a.score);
  return results.slice(0, limit).map(({ r }) => ({
    title: r.heading ? `${r.title} › ${r.heading}` : r.title,
    url: r.url,
    markdown_url: r.markdownUrl,
    excerpt: excerpt(r.content, tokens),
  }));
}

function excerpt(text, tokens, size = 280) {
  const lower = text.toLowerCase();
  let at = -1;
  for (const t of tokens) {
    at = lower.indexOf(t);
    if (at >= 0) break;
  }
  const start = Math.max(0, at - 60);
  const cut = text.slice(start, start + size).replace(/\s+/g, " ").trim();
  return (start > 0 ? "…" : "") + cut + (start + size < text.length ? "…" : "");
}
