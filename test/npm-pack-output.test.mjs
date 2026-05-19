import assert from "node:assert/strict";
import test from "node:test";
import { parseNpmPackJson } from "../src/adapters/cli/npm-pack-output.mjs";

test("parseNpmPackJson accepts plain npm pack json output", () => {
  const result = parseNpmPackJson(`[
  {
    "filename": "robotaccomplice-architext-1.3.0.tgz"
  }
]`);
  assert.equal(result[0].filename, "robotaccomplice-architext-1.3.0.tgz");
});

test("parseNpmPackJson ignores noisy lifecycle output with ANSI bracket sequences", () => {
  const result = parseNpmPackJson(`\u001b[36mvite v6.4.2 \u001b[32mbuilding for production...\u001b[39m
✓ built in 413ms
[
  {
    "filename": "robotaccomplice-architext-1.3.0.tgz"
  }
]`);
  assert.equal(result[0].filename, "robotaccomplice-architext-1.3.0.tgz");
});

test("parseNpmPackJson fails loud when npm pack json is missing", () => {
  assert.throws(
    () => parseNpmPackJson("vite build finished without npm json"),
    /Unable to parse npm pack --json output/
  );
});
