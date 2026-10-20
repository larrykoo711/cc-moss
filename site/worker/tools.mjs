// The MCP tools, defined once: the Worker serves them and the build writes the discovery files
// (server card, mcp.json, server.json) from the same list, so they can never disagree.

export const ORIGIN = "https://cc-moss.larrykoo.com";
export const SERVER_INFO = { name: "cc-moss-site", title: "cc-moss", version: "1.0.0" };
export const PROTOCOL_VERSIONS = ["2025-06-18", "2025-03-26", "2024-11-05"];

export const INSTRUCTIONS =
  "cc-moss is an open-source session manager for Claude Code (macOS and Linux, with a macOS menu bar app). " +
  "Use search_docs to find how something works, read_doc to read a whole page as Markdown, and get_release " +
  "for the latest version with install and update commands. Answer from these, and link the page you used.";

const READ_ONLY = { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false };

export const TOOLS = [
  {
    name: "search_docs",
    title: "Search the cc-moss docs",
    description:
      "Search the cc-moss site and guide (install, commands, the t picker, the menu bar app, Mo, Moss, archive, switching providers, notifications, configuration, privacy, FAQ, changelog). Returns the best-matching sections with their URLs.",
    inputSchema: {
      type: "object",
      properties: {
        query: { type: "string", description: "What to look for, in English or Chinese, e.g. 'restore after reboot' or '快捷键'." },
        locale: { type: "string", enum: ["en", "zh"], description: "Language of the results. Defaults to en." },
        limit: { type: "integer", minimum: 1, maximum: 10, description: "How many sections to return. Defaults to 5." },
      },
      required: ["query"],
      additionalProperties: false,
    },
    annotations: { title: "Search the cc-moss docs", ...READ_ONLY },
  },
  {
    name: "read_doc",
    title: "Read a cc-moss page",
    description:
      "Read a whole cc-moss page as Markdown: / (overview and FAQ), /changelog/ (every release in plain words) or /guide/ (the full guide); /zh/, /zh/changelog/ and /zh/guide/ for Chinese. A full URL from search_docs works too.",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string", description: "Page path or URL, e.g. /guide/, /zh/, https://cc-moss.larrykoo.com/guide/#install." },
      },
      required: ["path"],
      additionalProperties: false,
    },
    annotations: { title: "Read a cc-moss page", ...READ_ONLY },
  },
  {
    name: "get_release",
    title: "Latest cc-moss release",
    description:
      "The latest cc-moss release from GitHub: version, date, release notes, and the commands to install, update or uninstall it on macOS and Linux.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
    annotations: { title: "Latest cc-moss release", ...READ_ONLY, openWorldHint: true },
  },
];

/** SEP-1649 server card, with the registry fields some clients read instead. */
export function serverCard() {
  return {
    $schema: "https://static.modelcontextprotocol.io/schemas/mcp-server-card/v1.json",
    name: "com.larrykoo.cc-moss/site",
    title: SERVER_INFO.title,
    description: "Read the cc-moss docs and get the latest release, from any MCP client. Read-only, no sign-in.",
    version: SERVER_INFO.version,
    serverInfo: SERVER_INFO,
    protocolVersion: PROTOCOL_VERSIONS[0],
    websiteUrl: `${ORIGIN}/`,
    documentation: `${ORIGIN}/llms.txt`,
    repository: { url: "https://github.com/larrykoo711/cc-moss", source: "github", subfolder: "site" },
    icons: [{ src: `${ORIGIN}/icon.svg`, mimeType: "image/svg+xml" }],
    remotes: [{ type: "streamable-http", url: `${ORIGIN}/mcp`, supportedProtocolVersions: PROTOCOL_VERSIONS }],
    serverUrl: `${ORIGIN}/mcp`,
    transport: { type: "streamable-http", endpoint: `${ORIGIN}/mcp` },
    authentication: { required: false },
    capabilities: { tools: { listChanged: false } },
    tools: TOOLS.map(({ name, title, description, inputSchema, annotations }) => ({ name, title, description, inputSchema, annotations })),
  };
}

/** /.well-known/mcp.json: a short manifest pointing at the same server. */
export function mcpManifest() {
  return {
    name: "cc-moss",
    kind: "mcp-server",
    description: serverCard().description,
    url: `${ORIGIN}/mcp`,
    transport: "streamable-http",
    serverCard: `${ORIGIN}/.well-known/mcp/server-card.json`,
    capabilities: { tools: true },
    tools: TOOLS.map(({ name, description, inputSchema }) => ({ name, description, parameters: inputSchema })),
  };
}

/** /server.json for the MCP Registry. */
export function registryManifest() {
  const card = serverCard();
  return {
    $schema: "https://static.modelcontextprotocol.io/schemas/2025-09-29/server.schema.json",
    name: card.name,
    title: card.title,
    description: card.description,
    version: card.version,
    websiteUrl: card.websiteUrl,
    repository: card.repository,
    remotes: [{ type: "streamable-http", url: `${ORIGIN}/mcp` }],
  };
}
