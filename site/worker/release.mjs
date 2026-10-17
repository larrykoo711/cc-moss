// The latest release, read from GitHub's releases feed (the REST API rate-limits shared addresses).

/** The newest entry of a GitHub releases feed, with the commands that go with it. */
export function releaseFromFeed(xml) {
  const entry = xml.match(/<entry>([\s\S]*?)<\/entry>/)?.[1];
  if (!entry) throw new Error("no release in the feed");
  const pick = (re) => entry.match(re)?.[1] || "";
  const url = pick(/<link[^>]*href="([^"]+)"/);
  const unescape = (s) => s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, "&");
  const notes = unescape(pick(/<content[^>]*>([\s\S]*?)<\/content>/))
    .replace(/<li>/g, "- ")
    .replace(/<\/(p|li|h\d)>/g, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/-\s*\n+\s*/g, "- ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return {
    version: url.split("/tag/")[1]?.replace(/^v/, "") || unescape(pick(/<title>([^<]*)<\/title>/)),
    published: pick(/<updated>([^<]+)<\/updated>/),
    url,
    notes: notes.slice(0, 3000),
    install: {
      macos: "brew install larrykoo711/tap/cc-moss && cc-moss install",
      linux: "brew install larrykoo711/tap/cc-moss && cc-moss install   # x86_64; on arm64 download a binary from the release page",
      binaries: url,
    },
    update: "cc-moss update",
    uninstall: "cc-moss install --uninstall && brew uninstall cc-moss",
  };
}
