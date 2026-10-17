import assert from "node:assert/strict";
import { test } from "node:test";
import { acceptsNeither, isAiAgent, prefersMarkdown } from "../worker/negotiate.mjs";

test("a client that names text/markdown first gets Markdown", () => {
  assert.equal(prefersMarkdown("text/markdown"), true);
  assert.equal(prefersMarkdown("text/markdown, text/html;q=0.9"), true);
});

test("a tie goes to Markdown; a lower q does not", () => {
  assert.equal(prefersMarkdown("text/html, text/markdown"), true);
  assert.equal(prefersMarkdown("text/html, text/markdown;q=0.5"), false);
});

test("a browser's wildcard never selects Markdown", () => {
  assert.equal(prefersMarkdown("text/html,application/xhtml+xml,*/*;q=0.8"), false);
  assert.equal(prefersMarkdown("*/*"), false);
  assert.equal(prefersMarkdown(""), false);
});

test("q=0 refuses Markdown", () => {
  assert.equal(prefersMarkdown("text/markdown;q=0"), false);
});

test("a client that takes neither HTML nor Markdown is told so", () => {
  assert.equal(acceptsNeither("application/json"), true);
  assert.equal(acceptsNeither("image/png, application/json"), true);
  assert.equal(acceptsNeither("*/*"), false);
  assert.equal(acceptsNeither(null), false);
});

test("AI fetchers are recognised; search crawlers are not", () => {
  assert.equal(isAiAgent("Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; ClaudeBot/1.0; +claudebot@anthropic.com)"), true);
  assert.equal(isAiAgent("Mozilla/5.0 (compatible; ChatGPT-User/1.0; +https://openai.com/bot)"), true);
  assert.equal(isAiAgent("Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)"), false);
  assert.equal(isAiAgent("Mozilla/5.0 (compatible; bingbot/2.0)"), false);
  assert.equal(isAiAgent("Mozilla/5.0 (Macintosh) Safari/605.1.15"), false);
});
