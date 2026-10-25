import assert from "node:assert/strict";
import { test } from "node:test";
import { runSecurityAudit } from "../scripts/security-audit.mjs";

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
