// What AI assistants and agents read: llms.txt, the full text, search records for the MCP
// server, the agent skill and the catalogs that point at them. Only what exists is listed.
import { createHash } from "node:crypto";
import { ORIGIN, REPO, VERSION, localePath, url } from "./site.mjs";
import { mcpManifest, registryManifest, serverCard } from "../worker/tools.mjs";

const COPY = {
  en: {
    summary:
      "cc-moss is an open-source session manager for Claude Code. It tracks every session (busy, waiting for you, or stopped) and brings interrupted ones back to the same conversation after a restart or a crash, with one command or one click. Free (MIT), for macOS and Linux, with a menu bar app on macOS 14+, where it also keeps Moss running: a Claude Code session that keeps your sessions tidy and asks before anything risky. Every page has a Markdown version: append `index.md` to its URL or send `Accept: text/markdown`.",
    facts: [`Version: ${VERSION}`, "License: MIT, free", "Platforms: macOS (terminal + menu bar app, macOS 14+), Linux (terminal)", "Needs: Claude Code, tmux"],
    when: [
      "You run several Claude Code sessions at once and want to see which are busy, waiting for your approval, or stopped.",
      "Your Mac restarted, the terminal crashed or a window closed, and you want every interrupted conversation back in its own folder: `cc-moss restore` (or Bring them back in the menu bar).",
      "You want to know the moment Claude needs a yes: a notification, or Mo, a small desktop companion.",
      "You want to jump to any session fast: type `t` in a terminal, or double-tap Control for the menu bar panel.",
      "You want to answer a session's question or approve its request without going into its terminal: from the menu bar panel or Mo (macOS).",
      "You want old sessions archived on their own, or to switch API providers (with a tool like cc-switch) without starting your sessions over.",
    ],
    whenNot:
      "Do not use it for: Windows (not supported); restoring non-Claude terminal panes (use tmux-resurrect, which works alongside cc-moss); a one-off chat you never come back to.",
    how: "Install: `brew install larrykoo711/tap/cc-moss && cc-moss install` (x86_64 Linux too; arm64 Linux: download a binary from the releases page). Update: `cc-moss update`. Check the setup: `cc-moss doctor`.",
    pages: [
      ["Overview and FAQ", "/", "what cc-moss does, Moss, how it compares to claude --resume and tmux-resurrect, common questions"],
      ["Changelog", "/changelog/", "every release in plain words, newest first, with links to the technical notes"],
      ["Full guide", "/guide/", "every detail for assistants: install and update, the t picker and every command, the menu bar app, Mo and notifications, configuration, privacy, troubleshooting"],
    ],
    other: ["中文版", "/zh/llms.txt"],
    headings: { when: "When to use cc-moss", how: "How to get it", pages: "Pages", dev: "Developer resources", optional: "Optional" },
  },
  zh: {
    summary:
      "cc-moss 是开源的 Claude Code 会话管理工具。它照看每个会话（在忙、在等你、断了），电脑重启或终端崩溃后，一条命令或一次点击，把中断的会话接回原来的对话。免费（MIT），支持 macOS 和 Linux，macOS 14+ 上有状态栏 App，还会替你一直开着 Moss：一个帮你整理会话、动手前先问你的 Claude Code 会话。每个页面都有 Markdown 版本：在网址后加 `index.md`，或请求时带 `Accept: text/markdown`。",
    facts: [`版本：${VERSION}`, "许可：MIT，免费", "平台：macOS（终端 + 状态栏 App，需 macOS 14+）、Linux（终端）", "依赖：Claude Code、tmux"],
    when: [
      "同时开着好几个 Claude Code 会话，想一眼看清谁在忙、谁在等你点头、谁断了。",
      "电脑重启、终端崩溃或窗口关错了，想把每个中断的对话在原目录接回来：`cc-moss restore`（或状态栏里的「全部找回」）。",
      "想在 Claude 需要你确认时第一时间知道：系统通知，或者桌面上的小伙伴小莫。",
      "想快速跳到任意会话：终端里敲 `t`，或者连按两下 Control 唤出状态栏面板。",
      "想不进终端就回答会话的问题、批准它的请求：在状态栏面板或小莫那里点一下（macOS）。",
      "想让旧会话自己归档，或者用 cc-switch 这类工具换 API 供应商时不用把会话从头开过。",
    ],
    whenNot: "不适合：Windows（不支持）；恢复与 Claude 无关的终端面板（用 tmux-resurrect，可与 cc-moss 一起用）；只聊一次、不会再回来的对话。",
    how: "安装：`brew install larrykoo711/tap/cc-moss && cc-moss install`（x86_64 Linux 同样适用；arm64 Linux 请在发布页下载二进制）。更新：`cc-moss update`。检查环境：`cc-moss doctor`。",
    pages: [
      ["概览与常见问题", "/", "cc-moss 能做什么、Moss、和 claude --resume / tmux-resurrect 的对比、常见问题"],
      ["更新记录", "/changelog/", "每一版新增了什么，用大白话写，最新的在前，附技术记录链接"],
      ["完整手册", "/guide/", "供 AI 助手查阅的全部细节：安装与更新、t 选择器和全部命令、状态栏 App、小莫与通知、配置、隐私、排错"],
    ],
    other: ["English", "/llms.txt"],
    headings: { when: "什么时候用 cc-moss", how: "怎么获取", pages: "页面", dev: "开发者资源", optional: "可选" },
  },
};

const DEV = [
  ["MCP server", "/mcp", "Streamable HTTP, read-only, no sign-in; tools: search_docs, read_doc, get_release"],
  ["MCP server card", "/.well-known/mcp/server-card.json", ""],
  ["Agent skill", "/.well-known/agent-skills/cc-moss/SKILL.md", "how to install and use cc-moss, for coding agents"],
  ["Sitemap", "/sitemap.xml", ""],
];

export function llmsTxt(locale, updated) {
  const c = COPY[locale];
  const line = (name, href, note) => `- [${name}](${href})${note ? `: ${note}` : ""}`;
  return [
    "# cc-moss",
    "",
    `> ${c.summary}`,
    "",
    ...c.facts.map((f) => `- ${f}`),
    `- ${locale === "zh" ? "最后更新" : "Last updated"}: ${updated}`,
    "",
    `## ${c.headings.when}`,
    "",
    ...c.when.map((w) => `- ${w}`),
    "",
    c.whenNot,
    "",
    `## ${c.headings.how}`,
    "",
    c.how,
    "",
    `## ${c.headings.pages}`,
    "",
    ...c.pages.map(([name, path, note]) => line(name, url(`${localePath(locale, path)}index.md`), note)),
    "",
    `## ${c.headings.dev}`,
    "",
    ...DEV.map(([name, path, note]) => line(name, url(path), note)),
    line("Source code", REPO, "Rust CLI and Swift menu bar app, MIT"),
    line("Releases", `${REPO}/releases`, ""),
    "",
    `## ${c.headings.optional}`,
    "",
    line(locale === "zh" ? "全文" : "Full text", url(localePath(locale, "/llms-full.txt")), ""),
    line(c.other[0], url(c.other[1]), ""),
    "",
  ].join("\n");
}

/** Every page's Markdown, one after another, each under its URL. */
export function llmsFull(docs) {
  return docs.map(({ path, markdown }) => `---\n\nURL: ${url(path)}\n\n${stripFrontmatter(markdown).trim()}\n`).join("\n");
}

const stripFrontmatter = (md) => md.replace(/^---\n[\s\S]*?\n---\n/, "");

/** Split pages into sections (by H2/H3) for search_docs. `ids` are the anchors the HTML has. */
export function searchRecords(docs) {
  const out = [];
  for (const { locale, path, title, markdown, ids = new Set(), anchors = new Map(), agentOnly = false } of docs) {
    const body = stripFrontmatter(markdown);
    let heading = "";
    let anchor = "";
    let buf = [];
    const flush = () => {
      const content = buf.join("\n").replace(/\n{3,}/g, "\n\n").trim();
      // Agent-only documents have no HTML page: their address is the Markdown itself.
      const md = url(`${path}index.md`);
      const link = agentOnly ? md : url(path + (anchor ? `#${anchor}` : ""));
      if (content) out.push({ locale, title, heading, url: link, markdownUrl: md, content });
      buf = [];
    };
    for (const line of body.split("\n")) {
      const m = line.match(/^(#{2,3})\s+(.*)$/);
      if (m) {
        flush();
        heading = m[2].replace(/[`*_\\]/g, "").trim();
        // Link a section only when the page really has that anchor: the heading's own id, or one
        // derived from its words.
        anchor = anchors.get(heading) || (ids.has(slugOf(m[2])) ? slugOf(m[2]) : "");
      } else if (!/^#\s/.test(line)) buf.push(line);
    }
    flush();
  }
  return out;
}

const slugOf = (t) => t.toLowerCase().replace(/<[^>]+>/g, "").replace(/[^\p{L}\p{N}\s-]/gu, "").trim().replace(/\s+/g, "-");

// ---- discovery files ---------------------------------------------------------------------

export const SKILL_MD = `---
name: cc-moss
description: Install, use and troubleshoot cc-moss, the Claude Code session manager. Use when someone wants to see or recover their Claude Code sessions, bring sessions back after a restart or crash, or set up the cc-moss menu bar app.
---

# cc-moss

cc-moss tracks every Claude Code session (busy, waiting for you, stopped) and brings interrupted
ones back to the same conversation after a restart. macOS and Linux; menu bar app on macOS 14+.
Full guide (Markdown): ${ORIGIN}/guide/index.md. MCP server: ${ORIGIN}/mcp.

## Install

1. \`brew install larrykoo711/tap/cc-moss\` (macOS, x86_64 Linux). On arm64 Linux, download a binary from ${REPO}/releases/latest.
2. \`cc-moss install\` connects it to Claude Code (hooks), sets up tmux, the prompt and the \`t\` alias. It asks before changing anything; \`--dry-run\` shows the plan.
3. \`cc-moss doctor\` checks the setup and says how to fix anything missing.

## Everyday use

- \`t\`: pick a session and jump to it (Return on a stopped one brings it back).
- \`cc-moss ls\`: list sessions; \`cc-moss ls --json\` for scripts.
- \`cc-moss restore\`: bring back every interrupted session (\`--dry-run\` to preview).
- \`cc-moss new NAME -C DIR\`: start a named session in its own tmux session.
- Menu bar app: double-tap Control for the panel; ⌘K for actions.

## Update and remove

- Update: \`cc-moss update\` (or \`brew upgrade cc-moss\`).
- Uninstall: \`cc-moss install --uninstall\`, then \`brew uninstall cc-moss\`.

## When something is off

- \`t: command not found\`: \`~/.local/bin\` must be on PATH; open a new terminal or run \`cc-moss doctor\`.
- After an upgrade \`doctor\` reports missing hooks: run \`cc-moss setup\`.
- Privacy: everything stays local; the only network request is a daily version check, which can be turned off (\`CC_MOSS_NO_UPDATE_CHECK=1\`).
`;

export const sha256 = (text) => createHash("sha256").update(text).digest("hex");

export function skillsIndex() {
  return {
    $schema: "https://schemas.agentskills.io/discovery/0.2.0/schema.json",
    skills: [
      {
        name: "cc-moss",
        type: "skill-md",
        description: SKILL_MD.match(/^description: (.*)$/m)[1],
        url: url("/.well-known/agent-skills/cc-moss/SKILL.md"),
        digest: `sha256:${sha256(SKILL_MD)}`,
      },
    ],
  };
}

export function aiCatalog() {
  return {
    specVersion: "1.0",
    entries: [
      {
        identifier: "urn:air:cc-moss.larrykoo.com:mcp:site",
        displayName: "cc-moss docs and releases (MCP)",
        description: "Read-only MCP server: search the cc-moss docs, read any page as Markdown, get the latest release.",
        type: "application/mcp-server-card+json",
        url: url("/.well-known/mcp/server-card.json"),
        trustManifest: { identity: "urn:air:cc-moss.larrykoo.com:mcp:site", identityType: "dns", privacyPolicyUrl: url("/guide/#privacy") },
      },
    ],
  };
}

/** Every machine-readable file, path → content. */
export function discoveryFiles() {
  const json = (o) => `${JSON.stringify(o, null, 2)}\n`;
  return {
    ".well-known/mcp.json": json(mcpManifest()),
    ".well-known/mcp/server-card.json": json(serverCard()),
    "server.json": json(registryManifest()),
    ".well-known/agent-skills/index.json": json(skillsIndex()),
    ".well-known/agent-skills/cc-moss/SKILL.md": SKILL_MD,
    ".well-known/ai-catalog.json": json(aiCatalog()),
  };
}
