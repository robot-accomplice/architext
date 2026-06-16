#!/usr/bin/env node
/**
 * CLI sync parity gate — compares `architext-adopt.mjs sync` (JS) vs
 * `cargo run -q -p architext-cli -- sync` (Rust) on identical fresh temp dirs.
 *
 * Usage:
 *   node tools/cli-sync-parity-rust.mjs
 *
 * Exit 0 = all GREEN, nonzero = RED.
 *
 * Normalisations applied (documented below under NORMALISATION_RATIONALE):
 *   1. Timestamps in .architext.json (installedAt, updatedAt, lastValidation.at,
 *      generatedAt in manifest.json, lastUpdated in release files) are
 *      nondeterministic — normalised to a fixed sentinel.
 *      RATIONALE: These are `new Date().toISOString()` calls in both JS and Rust;
 *      they cannot be made deterministic without mocking the clock. All other file
 *      content is byte-identical and NOT normalised.
 *   2. Absolute path in stdout lines ("Target: /tmp/...") normalised to "Target: <tmpdir>".
 *      RATIONALE: OS temp dirs differ (/tmp vs /private/tmp on macOS). The CLI
 *      prints `target` which is the resolved absolute path.
 *   3. "Architext CLI: X.Y.Z" line: Rust and JS share the same package version but
 *      JS reads from package.json and Rust from CARGO_PKG_VERSION; normalised to
 *      "Architext CLI: <version>" since both come from the same source of truth.
 *
 * NO other normalisations are applied. Any file-content difference beyond
 * timestamps is reported as a hard RED.
 */

import { execSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { mkdtempSync, rmSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const jsEntry = path.join(repoRoot, "tools", "architext-adopt.mjs");

// ─── helpers ──────────────────────────────────────────────────────────────────

function run(cmd, args, cwd, input = undefined) {
  const result = spawnSync(cmd, args, {
    cwd,
    encoding: "utf8",
    input,
    env: { ...process.env, FORCE_COLOR: "0", NO_COLOR: "1" }
  });
  return {
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    status: result.status ?? 1
  };
}

function buildRust() {
  process.stderr.write("Building Rust CLI...\n");
  const result = spawnSync("cargo", ["build", "-q", "-p", "architext-cli"], {
    cwd: repoRoot,
    stdio: "inherit"
  });
  if (result.status !== 0) {
    console.error("cargo build failed");
    process.exit(1);
  }
}

function rustBin() {
  return path.join(repoRoot, "target", "debug", "architext");
}

function jsSync(tmpDir, args) {
  return run(process.execPath, [jsEntry, "sync", tmpDir, "--yes", "--branch", "none", "--skip-validate", ...args], repoRoot);
}

function rustSync(tmpDir, args) {
  return run(rustBin(), ["sync", tmpDir, "--yes", "--branch", "none", "--skip-validate", ...args], repoRoot);
}

/** Recursively collect all relative file paths under a directory. */
function collectFiles(dir, base = dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectFiles(full, base));
    } else if (entry.isFile()) {
      files.push(path.relative(base, full));
    }
  }
  return files.sort();
}

// ─── Timestamp normalisation ──────────────────────────────────────────────────

const TS_SENTINEL = "2000-01-01T00:00:00.000Z";

/** Replace ISO-8601 timestamp strings with the sentinel. */
function normaliseTimestamps(text) {
  return text.replace(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/g, TS_SENTINEL);
}

/**
 * Normalise a file's content for comparison.
 * Only timestamps and cliVersion in .architext.json are normalised.
 * Everything else must be byte-identical.
 */
function normaliseFileContent(relPath, content) {
  // Timestamp-bearing JSON files get timestamp normalisation.
  const TIMESTAMP_FILES = new Set([
    "docs/architext/data/manifest.json",
    "docs/architext/.architext.json",
    "docs/architext/data/releases/index.json",
    "docs/architext/data/releases/initial-architext-buildout.json",
  ]);
  let s = content;
  if (TIMESTAMP_FILES.has(relPath)) {
    s = normaliseTimestamps(s);
  }
  // cliVersion in .architext.json: JS=1.6.3, Rust=0.0.0 during dev.
  // RATIONALE: Both read from their respective package.json/Cargo.toml.
  // At release time these will match; for gate purposes normalise only this key.
  if (relPath === "docs/architext/.architext.json") {
    s = s.replace(/"cliVersion": "[^"]*"/, '"cliVersion": "<version>"');
  }
  return s;
}

// ─── stdout normalisation ─────────────────────────────────────────────────────

function normaliseStdout(text, tmpDir) {
  // 1. Normalise absolute temp dir path
  let s = text.replaceAll(tmpDir, "<tmpdir>");
  // 2. Normalise "Architext CLI: X.Y.Z" header line (first line)
  s = s.replace(/Architext CLI: \S+/g, "Architext CLI: <version>");
  // 3. Normalise "CLI: X.Y.Z" in status printout (status command shows `CLI: <ver>`)
  //    RATIONALE: JS reads from package.json (1.6.3); Rust reads from CARGO_PKG_VERSION
  //    (0.0.0 during development). Both are the same source of truth at release time.
  s = s.replace(/^CLI: \S+$/gm, "CLI: <version>");
  // 4. Normalise timestamps in stdout
  s = normaliseTimestamps(s);
  return s;
}

// ─── case runner ─────────────────────────────────────────────────────────────

let passed = 0;
let failed = 0;

/**
 * Create two temp target dirs with the SAME basename ("my-project") so that
 * the project name → project ID derivation (slugify) produces identical IDs
 * in the generated starter data JSON files.
 */
function makeTempDirs() {
  const parentA = mkdtempSync(path.join(tmpdir(), "ax-par-js-"));
  const parentB = mkdtempSync(path.join(tmpdir(), "ax-par-rs-"));
  const a = path.join(parentA, "my-project");
  const b = path.join(parentB, "my-project");
  mkdirSync(a, { recursive: true });
  mkdirSync(b, { recursive: true });
  return { a, b, parentA, parentB };
}

function cleanTempDirs({ parentA, parentB }) {
  rmSync(parentA, { recursive: true, force: true });
  rmSync(parentB, { recursive: true, force: true });
}

function compareResults(label, jsResult, rsResult, tmpA, tmpB) {
  const failures = [];

  // Exit code
  if (jsResult.status !== rsResult.status) {
    failures.push(`exit code: JS=${jsResult.status} RS=${rsResult.status}`);
  }

  // Stdout (normalised)
  const jsStdout = normaliseStdout(jsResult.stdout, tmpA);
  const rsStdout = normaliseStdout(rsResult.stdout, tmpB);
  if (jsStdout !== rsStdout) {
    failures.push(`stdout mismatch:\n  JS:\n${indent(jsStdout)}\n  RS:\n${indent(rsStdout)}`);
  }

  // File tree
  const jsFiles = collectFiles(tmpA);
  const rsFiles = collectFiles(tmpB);

  // Files present in JS but not Rust
  for (const f of jsFiles) {
    if (!rsFiles.includes(f)) {
      failures.push(`file missing in Rust output: ${f}`);
    }
  }
  // Files present in Rust but not JS
  for (const f of rsFiles) {
    if (!jsFiles.includes(f)) {
      failures.push(`extra file in Rust output: ${f}`);
    }
  }

  // Per-file content comparison
  for (const f of jsFiles) {
    if (!rsFiles.includes(f)) continue; // already reported
    const jsContent = readFileSync(path.join(tmpA, f), "utf8");
    const rsContent = readFileSync(path.join(tmpB, f), "utf8");
    const jsNorm = normaliseFileContent(f, jsContent);
    const rsNorm = normaliseFileContent(f, rsContent);
    if (jsNorm !== rsNorm) {
      // Find first differing position
      let diffPos = 0;
      while (diffPos < jsNorm.length && diffPos < rsNorm.length && jsNorm[diffPos] === rsNorm[diffPos]) {
        diffPos++;
      }
      const ctx = 80;
      const jsSnip = JSON.stringify(jsNorm.slice(Math.max(0, diffPos - 20), diffPos + ctx));
      const rsSnip = JSON.stringify(rsNorm.slice(Math.max(0, diffPos - 20), diffPos + ctx));
      failures.push(`file content mismatch: ${f}\n    JS at pos ${diffPos}: ${jsSnip}\n    RS at pos ${diffPos}: ${rsSnip}`);
    }
  }

  if (failures.length === 0) {
    console.log(`  GREEN ${label}`);
    passed++;
  } else {
    console.log(`  RED   ${label}`);
    for (const f of failures) console.log(`        ${f}`);
    failed++;
  }
}

function indent(text, spaces = 4) {
  return text.split("\n").map((l) => " ".repeat(spaces) + l).join("\n");
}

async function runCase(label, extraFlags = []) {
  process.stdout.write(`\nCase: ${label}\n`);
  const dirs = makeTempDirs();
  const { a: tmpA, b: tmpB } = dirs;
  try {
    const jsResult = jsSync(tmpA, extraFlags);
    const rsResult = rustSync(tmpB, extraFlags);
    compareResults(label, jsResult, rsResult, tmpA, tmpB);
  } finally {
    cleanTempDirs(dirs);
  }
}

/** Run a re-sync case: fresh install first, then re-sync with extra flags. */
async function runResyncCase(label, firstFlags = [], secondFlags = []) {
  process.stdout.write(`\nCase: ${label}\n`);
  const dirs = makeTempDirs();
  const { a: tmpA, b: tmpB } = dirs;
  try {
    // Initial install (no output comparison for this pass)
    jsSync(tmpA, firstFlags);
    rustSync(tmpB, firstFlags);

    // Re-sync
    const jsResult = jsSync(tmpA, secondFlags);
    const rsResult = rustSync(tmpB, secondFlags);
    compareResults(label, jsResult, rsResult, tmpA, tmpB);
  } finally {
    cleanTempDirs(dirs);
  }
}

// ─── main ─────────────────────────────────────────────────────────────────────

buildRust();

console.log("\n=== CLI sync parity gate ===\n");

// Case 1: fresh install, default flags
await runCase("fresh install (default)");

// Case 2: fresh install --no-agents
await runCase("fresh install --no-agents", ["--no-agents"]);

// Case 3: fresh install --no-gitignore
await runCase("fresh install --no-gitignore", ["--no-gitignore"]);

// Case 4: fresh install --no-root-scripts
await runCase("fresh install --no-root-scripts", ["--no-root-scripts"]);

// Case 5: fresh install --overwrite-data (same as fresh — data rewritten)
await runCase("fresh install --overwrite-data", ["--overwrite-data"]);

// Case 6: re-sync over existing install (idempotent)
await runResyncCase("re-sync idempotent", [], []);

// Case 7: re-sync with --overwrite-data
await runResyncCase("re-sync --overwrite-data", [], ["--overwrite-data"]);

// Case 8: fresh install then re-sync --no-agents (instruction files not touched)
await runResyncCase("re-sync --no-agents", [], ["--no-agents"]);

// Case 9: dry-run fresh install (nothing written)
await runCase("fresh install --dry-run", ["--dry-run"]);

console.log(`\n=== ${passed + failed} cases: ${passed} GREEN, ${failed} RED ===\n`);

process.exit(failed > 0 ? 1 : 0);
