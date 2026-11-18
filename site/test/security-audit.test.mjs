import assert from "node:assert/strict";
import { test } from "node:test";
import { runSecurityAudit, WAIVERS } from "../scripts/security-audit.mjs";

const clean = () => ({
  advisories: {},
  metadata: { vulnerabilities: { info: 0, low: 0, moderate: 0, high: 0, critical: 0 } },
});

function audit(result) {
  const messages = [];
  const code = runSecurityAudit({
    run(command, args, options) {
      assert.equal(command, "pnpm");
      assert.deepEqual(args, ["audit", "--json", "--registry=https://registry.npmjs.org"]);
      assert.equal(options.shell, false);
      assert.ok(options.timeout > 0);
      return result;
    },
    log: (message) => messages.push(message),
  });
  return { code, output: messages.join("\n") };
}

function response(body, status = 0) {
  return { status, signal: null, stdout: JSON.stringify(body), stderr: "" };
}

test("a successful scan with no vulnerabilities passes with a readable summary", () => {
  const result = audit(response(clean()));
  assert.equal(result.code, 0);
  assert.match(result.output, /0 vulnerabilities/);
});

for (const status of [1, 7]) {
  test(`vulnerabilities fail and preserve child exit status ${status}`, () => {
    const body = clean();
    body.metadata.vulnerabilities.high = 2;
    body.advisories = { 123: { severity: "high" } };
    const result = audit(response(body, status));
    assert.equal(result.code, status);
    assert.match(result.output, /2 vulnerabilities.*high=2/);
  });
}

test("a finding still fails when the child incorrectly exits zero", () => {
  const body = clean();
  body.metadata.vulnerabilities.low = 1;
  assert.notEqual(audit(response(body)).code, 0);
});

test("advisories cannot be hidden by zero metadata counts", () => {
  const body = clean();
  body.advisories = { 123: { severity: "high" } };
  assert.notEqual(audit(response(body)).code, 0);
});

test("an API error is not confused with a vulnerability report", () => {
  const result = audit(response({ error: { code: "ECONNRESET", message: "registry unavailable" } }, 1));
  assert.equal(result.code, 1);
  assert.match(result.output, /scan failed/i);
  assert.doesNotMatch(result.output, /0 vulnerabilities/);
});

test("a nonzero child status cannot be hidden by a clean report", () => {
  assert.equal(audit(response(clean(), 23)).code, 23);
});

for (const [name, stdout] of [
  ["empty", ""], ["whitespace", "  \n"], ["malformed JSON", "{"],
  ["HTML error", "<html>502</html>"], ["null", "null"], ["array", "[]"],
  ["missing metadata", '{"advisories":{}}'],
  ["missing advisory data", JSON.stringify({ metadata: clean().metadata })],
  ["missing severity counts", '{"advisories":{},"metadata":{"vulnerabilities":{}}}'],
]) {
  test(`${name} response fails even with exit zero`, () => {
    assert.notEqual(audit({ status: 0, stdout, stderr: "" }).code, 0);
  });
}

for (const value of [-1, 0.5, "0", null]) {
  test(`invalid severity count ${JSON.stringify(value)} fails closed`, () => {
    const body = clean();
    body.metadata.vulnerabilities.high = value;
    assert.notEqual(audit(response(body)).code, 0);
  });
}

test("unknown severity counts fail closed", () => {
  const body = clean();
  body.metadata.vulnerabilities.unknown = 1;
  assert.notEqual(audit(response(body)).code, 0);
});

test("an error alongside otherwise clean metadata fails closed", () => {
  assert.notEqual(audit(response({ ...clean(), error: "registry error" })).code, 0);
});

test("spawn failure reports the error", () => {
  const result = audit({ status: null, error: new Error("spawn corepack ENOENT"), stdout: "", stderr: "" });
  assert.notEqual(result.code, 0);
  assert.match(result.output, /ENOENT/);
});

test("a killed or timed out audit cannot pass with clean output", () => {
  assert.notEqual(audit({ ...response(clean()), status: null, signal: "SIGTERM" }).code, 0);
});

test("malformed output preserves failure status and stderr", () => {
  const result = audit({ status: 9, stdout: "", stderr: "registry request failed" });
  assert.equal(result.code, 9);
  assert.match(result.output, /registry request failed/);
});

// Waivers: one reviewed advisory with no fix yet, until a date.
const waiver = { id: "GHSA-test-0000-0000", module: "picomatch", expires: "2026-12-18", reason: "build-time only" };
const before = new Date("2026-11-20T00:00:00Z");

function waivable(extra = {}) {
  const body = clean();
  body.metadata.vulnerabilities.high = 1;
  body.advisories = { 1: { github_advisory_id: waiver.id, module_name: "picomatch", severity: "high", patched_versions: null, ...extra } };
  return body;
}

function auditWith(result, { now = before, waivers = [waiver] } = {}) {
  const messages = [];
  const code = runSecurityAudit({ run: () => result, log: (m) => messages.push(m), now, waivers });
  return { code, output: messages.join("\n") };
}

test("a waived advisory with no fix passes and says it was waived", () => {
  const result = auditWith(response(waivable(), 1));
  assert.equal(result.code, 0);
  assert.match(result.output, /waived.*GHSA-test-0000-0000.*2026-12-18/);
});

test("a waiver stops working on its expiry date", () => {
  assert.notEqual(auditWith(response(waivable(), 1), { now: new Date("2026-12-18T00:00:00Z") }).code, 0);
});

for (const patched of [">=4.0.8", ">=2.3.3 <3.0.0"]) {
  test(`a waiver stops working once a fix exists (${patched})`, () => {
    assert.notEqual(auditWith(response(waivable({ patched_versions: patched }), 1)).code, 0);
  });
}

test("a waiver for one package does not cover another with the same id", () => {
  assert.notEqual(auditWith(response(waivable({ module_name: "micromatch" }), 1)).code, 0);
});

test("an unwaived advisory alongside a waived one still fails", () => {
  const body = waivable();
  body.metadata.vulnerabilities.moderate = 1;
  body.advisories[2] = { github_advisory_id: "GHSA-other", module_name: "other", severity: "moderate", patched_versions: ">=1.0.1" };
  const result = auditWith(response(body, 1));
  assert.notEqual(result.code, 0);
  assert.match(result.output, /GHSA-other/);
});

test("the shipped waivers each name a reason and an expiry", () => {
  for (const w of WAIVERS) {
    assert.match(w.id, /^GHSA-/);
    assert.ok(w.module && w.reason.length > 20, w.id);
    assert.ok(!Number.isNaN(Date.parse(w.expires)), w.id);
  }
});
