// Differential CONFORMANCE GATE (Phase validation parity): for each fixture
// directory in the corpus, run the JS validator and the Rust validator, compare
// the ACCEPT/REJECT verdict.
//
// JS verdict : spawn `node viewer/tools/validate-architext.mjs --data-dir <dir>`
//              exit 0 = accept, non-zero = reject.
// Rust verdict: `cargo run -q -p architext-core --bin validate -- <dir>`
//               stdout JSON {ok: bool, errors: [...]}; ok=true = accept.
//
// The stub Rust validator always accepts, so:
//   VALID fixtures   → MATCH  (both accept)
//   INVALID fixtures → MISMATCH (JS rejects, Rust accepts)
// This is the correct RED baseline. Subsequent validator-port passes drive
// all invalid fixtures to MATCH (Rust rejects), reaching 100 % GREEN.
//
// Exit non-zero if any MISMATCH is present.

import { execFileSync, spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import fs from "node:fs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const validateJs = path.join(repoRoot, "viewer", "tools", "validate-architext.mjs");
const corpusRoot = path.join(repoRoot, "crates", "architext-core", "tests", "conformance");

// ---------------------------------------------------------------------------
// Corpus: list of { label, dir, expectedJs } entries.
//
// VALID cases: point directly at real data dirs (no copy needed).
// INVALID cases: named subdirs under the conformance directory.
// ---------------------------------------------------------------------------
const VALID_DIRS = [
  { label: "docs/architext/data", dir: path.join(repoRoot, "docs", "architext", "data") }
];

function listInvalidFixtures() {
  const entries = fs.readdirSync(corpusRoot, { withFileTypes: true });
  return entries
    .filter((e) => e.isDirectory() && e.name.startsWith("invalid-"))
    .map((e) => ({ label: e.name, dir: path.join(corpusRoot, e.name) }));
}

const corpus = [
  ...VALID_DIRS.map((f) => ({ ...f, expectedJs: "accept" })),
  ...listInvalidFixtures().map((f) => ({ ...f, expectedJs: "reject" }))
];

// ---------------------------------------------------------------------------
// JS verdict
// ---------------------------------------------------------------------------
function jsVerdict(dir) {
  const result = spawnSync(
    process.execPath,
    [validateJs, "--data-dir", dir],
    { encoding: "utf8" }
  );
  return result.status === 0 ? "accept" : "reject";
}

// ---------------------------------------------------------------------------
// Rust verdict — cargo run -q -p architext-core --bin validate -- <dir>
// ---------------------------------------------------------------------------
function rustVerdict(dir) {
  const result = spawnSync(
    "cargo",
    ["run", "-q", "-p", "architext-core", "--bin", "validate", "--", dir],
    { encoding: "utf8", cwd: repoRoot }
  );
  if (result.status !== 0) {
    throw new Error(`cargo run failed (status ${result.status}): ${result.stderr}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(result.stdout.trim());
  } catch {
    throw new Error(`Rust validator emitted non-JSON: ${result.stdout}`);
  }
  return parsed.ok ? "accept" : "reject";
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------
console.log("Building Rust validator (first run may be slow)...");
// Pre-build so per-fixture runs use cached binary.
execFileSync("cargo", ["build", "-q", "-p", "architext-core", "--bin", "validate"], {
  cwd: repoRoot,
  stdio: "inherit"
});
console.log();

const rows = [];
let matches = 0;

for (const { label, dir, expectedJs } of corpus) {
  const js = jsVerdict(dir);
  const rust = rustVerdict(dir);
  const verdict = js === rust ? "MATCH" : "MISMATCH";
  if (verdict === "MATCH") matches += 1;

  const jsExpected = js === expectedJs ? "" : ` (expected ${expectedJs})`;
  rows.push({ label, js: js + jsExpected, rust, verdict });
}

// Print table
const colWidths = {
  label: Math.max(7, ...rows.map((r) => r.label.length)),
  js: Math.max(2, ...rows.map((r) => r.js.length)),
  rust: Math.max(4, ...rows.map((r) => r.rust.length)),
  verdict: 8
};

function pad(s, n) { return s.padEnd(n); }
function header() {
  return `${pad("FIXTURE", colWidths.label)}  ${pad("JS", colWidths.js)}  ${pad("RUST", colWidths.rust)}  VERDICT`;
}
function sep() {
  return `${"-".repeat(colWidths.label)}  ${"-".repeat(colWidths.js)}  ${"-".repeat(colWidths.rust)}  --------`;
}

console.log(header());
console.log(sep());
for (const { label, js, rust, verdict } of rows) {
  console.log(`${pad(label, colWidths.label)}  ${pad(js, colWidths.js)}  ${pad(rust, colWidths.rust)}  ${verdict}`);
}
console.log();
console.log(`Result: ${matches}/${corpus.length} MATCH`);
console.log();

// Report mismatches
const mismatches = rows.filter((r) => r.verdict === "MISMATCH");
if (mismatches.length > 0) {
  console.log("MISMATCHES (JS rejects but Rust accepts — expected for the stub RED baseline):");
  for (const { label } of mismatches) {
    console.log(`  - ${label}`);
  }
  console.log();
  console.log("EXIT 1: conformance gate not yet GREEN (expected — drive to 100% via validator port).");
  process.exit(1);
}

console.log("All fixtures MATCH — conformance gate GREEN.");
