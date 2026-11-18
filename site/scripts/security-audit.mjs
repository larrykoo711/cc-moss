// Every dependency, dev tools included, checked against the npm registry's advisories: `node scripts/security-audit.mjs`.
// CI runs it before building, so a known vulnerability stops a deploy; a scan that could not run fails too.
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const severities = ["info", "low", "moderate", "high", "critical"];
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

// Reviewed advisories that have no fixed version yet. A waiver covers one advisory in one package,
// ends on its expiry date, and ends as soon as the registry lists a fixed version.
export const WAIVERS = [
  {
    id: "GHSA-fc5q-39xc-vjpf",
    module: "picomatch",
    expires: "2026-12-18",
    reason: "ReDoS in extglob matching. Reached only through @tailwindcss/cli > @parcel/watcher > micromatch, " +
      "a build-time file watcher with fixed patterns; nothing on the site runs it. No fixed version exists (2026-11-18).",
  },
];

const hasFix = (advisory) => {
  const patched = typeof advisory.patched_versions === "string" ? advisory.patched_versions.trim() : "";
  return patched !== "" && patched !== "<0.0.0";
};

const waiverFor = (advisory, waivers, now) =>
  waivers.find((w) => w.id === advisory?.github_advisory_id && w.module === advisory?.module_name &&
    now < new Date(`${w.expires}T00:00:00Z`) && !hasFix(advisory));

export function runSecurityAudit({ run = spawnSync, log = console.log, now = new Date(), waivers = WAIVERS } = {}) {
  const result = run("pnpm", ["audit", "--json", "--registry=https://registry.npmjs.org"], {
    encoding: "utf8",
    shell: false,
    timeout: 120_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  // Preserve audit"s nonzero status, including registry failures, not just findings.
  const failureCode = Number.isInteger(result.status) && result.status > 0 ? result.status : 1;
  const fail = (reason) => {
    log(`Security audit scan failed: ${reason}`);
    if (result.stderr?.trim()) log(result.stderr.trim());
    return failureCode;
  };
  if (result.error || result.signal || !Number.isInteger(result.status)) {
    return fail(result.error?.message ?? result.signal ?? "missing child exit status");
  }

  let report;
  try {
    report = JSON.parse(result.stdout);
  } catch {
    return fail("empty or malformed JSON response");
  }
  if (!isObject(report) || "error" in report) return fail("registry returned an error or invalid report");

  const counts = report.metadata?.vulnerabilities;
  if (!isObject(counts) || Object.keys(counts).length !== severities.length ||
      !severities.every((severity) => Number.isSafeInteger(counts[severity]) && counts[severity] >= 0) ||
      !isObject(report.advisories)) {
    return fail("missing or invalid vulnerability counts/advisories");
  }
  const total = severities.reduce((sum, severity) => sum + counts[severity], 0);
  const advisoryCount = Object.keys(report.advisories).length;
  const advisories = Object.values(report.advisories);
  const open = advisories.filter((a) => !waiverFor(a, waivers, now));
  if (advisoryCount > 0 && open.length === 0) {
    for (const a of advisories) {
      const w = waiverFor(a, waivers, now);
      log(`Security audit: waived ${w.id} (${w.module}) until ${w.expires}: ${w.reason}`);
    }
    log(`Security audit: ${total} vulnerabilities, all from waived advisories; nothing else found.`);
    return 0;
  }
  if (total > 0 || advisoryCount > 0) {
    for (const a of open) log(`Security audit: ${a?.github_advisory_id ?? "advisory"} (${a?.module_name ?? "?"}, ${a?.severity ?? "?"})`);
    log(`Security audit: ${total} vulnerabilities (${severities.map((s) => `${s}=${counts[s]}`).join(", ")}); ${advisoryCount} advisories.`);
    return failureCode;
  }
  if (result.status !== 0) return fail(`audit exited ${result.status} despite zero reported findings`);
  log("Security audit: 0 vulnerabilities (npm official registry; all dependency scopes).");
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = runSecurityAudit();
}
