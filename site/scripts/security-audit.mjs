// Every dependency, dev tools included, checked against the npm registry's advisories: `node scripts/security-audit.mjs`.
// CI runs it before building, so a known vulnerability stops a deploy; a scan that could not run fails too.
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const severities = ["info", "low", "moderate", "high", "critical"];
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

export function runSecurityAudit({ run = spawnSync, log = console.log } = {}) {
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
  if (total > 0 || advisoryCount > 0) {
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
