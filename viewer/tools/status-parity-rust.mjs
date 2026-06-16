#!/usr/bin/env node
/**
 * status-parity-rust.mjs — differential gate for collectStatus JS ↔ Rust parity.
 *
 * Oracle: node tools/architext-adopt.mjs status <target> --json
 * Candidate: cargo run -q -p architext-core --bin status_dump -- <target> --version <v>
 *
 * Normalised fields (environment-dependent, not parity-relevant):
 *   - target, dataDir          : absolute paths — blank to "<NORMALIZED>"
 *   - cliVersion               : passed explicitly to both sides (matched)
 *   - metadata.installedAt, metadata.updatedAt, metadata.lastValidation.at
 *                              : timestamps — replaced with "<TIMESTAMP>"
 *   - validation.output        : JS validator vs Rust validator wording differs;
 *                                only validation.ok (bool) is compared.
 *
 * Exit 1 on any RED target.
 */

import { spawnSync, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");
const cliEntry = path.join(repoRoot, "tools", "architext-adopt.mjs");

// ─── The version string we hand to both sides ────────────────────────────────
// Derive from package.json so both oracles receive the same string.
const pkgJson = JSON.parse(fs.readFileSync(path.join(repoRoot, "package.json"), "utf8"));
const VERSION = pkgJson.version;

// ─── Helpers ─────────────────────────────────────────────────────────────────

function runOracle(target) {
  const result = spawnSync(
    process.execPath,
    [cliEntry, "status", target, "--json"],
    { encoding: "utf8", cwd: repoRoot }
  );
  if (result.status !== 0 && !result.stdout?.trim()) {
    throw new Error(`Oracle failed (status ${result.status}): ${result.stderr}`);
  }
  try {
    return JSON.parse(result.stdout.trim());
  } catch {
    throw new Error(`Oracle emitted non-JSON: ${result.stdout}`);
  }
}

function runCandidate(target) {
  const result = spawnSync(
    "cargo",
    ["run", "-q", "-p", "architext-core", "--bin", "status_dump", "--", target, "--version", VERSION],
    { encoding: "utf8", cwd: repoRoot }
  );
  if (result.status !== 0) {
    throw new Error(`Candidate failed (status ${result.status}): ${result.stderr}\nstdout: ${result.stdout}`);
  }
  try {
    return JSON.parse(result.stdout.trim());
  } catch {
    throw new Error(`Candidate emitted non-JSON stdout: ${result.stdout}`);
  }
}

/** Deep-normalize an object in-place for comparison. */
function normalize(obj) {
  if (obj === null || typeof obj !== "object") return obj;
  if (Array.isArray(obj)) return obj.map(normalize);

  const out = {};
  for (const [k, v] of Object.entries(obj)) {
    if (k === "target" || k === "dataDir") {
      out[k] = "<NORMALIZED>";
    } else if (k === "cliVersion") {
      // Both sides receive the same version so this should match; still normalize
      // to avoid surprising mismatches from packaging differences.
      out[k] = "<NORMALIZED>";
    } else if (k === "installedAt" || k === "updatedAt") {
      out[k] = v === null || v === undefined ? v : "<TIMESTAMP>";
    } else if (k === "at" && typeof v === "string") {
      // lastValidation.at
      out[k] = "<TIMESTAMP>";
    } else if (k === "output") {
      // validation.output — wording differs between JS and Rust validators.
      // We compare only validation.ok (bool); drop output.
      // Do NOT emit the key so both sides are omitted consistently.
    } else {
      out[k] = normalize(v);
    }
  }
  return out;
}

function deepEqual(a, b) {
  if (a === b) return true;
  if (a === null || b === null) return a === b;
  if (typeof a !== typeof b) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return false;
    return a.every((v, i) => deepEqual(v, b[i]));
  }
  if (typeof a === "object") {
    const ka = Object.keys(a).sort();
    const kb = Object.keys(b).sort();
    if (!deepEqual(ka, kb)) return false;
    return ka.every((k) => deepEqual(a[k], b[k]));
  }
  return false;
}

/** Return first divergent path string (breadcrumb) or null if equal. */
function firstDiff(a, b, path = "") {
  if (deepEqual(a, b)) return null;
  if (typeof a !== typeof b || a === null || b === null) {
    return `${path || "ROOT"}: JS=${JSON.stringify(a)} RUST=${JSON.stringify(b)}`;
  }
  if (Array.isArray(a) && Array.isArray(b)) {
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      const d = firstDiff(a[i], b[i], `${path}[${i}]`);
      if (d) return d;
    }
  }
  if (typeof a === "object" && !Array.isArray(a)) {
    const allKeys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const k of allKeys) {
      const d = firstDiff(a[k], b[k], path ? `${path}.${k}` : k);
      if (d) return d;
    }
  }
  return `${path || "ROOT"}: JS=${JSON.stringify(a)} RUST=${JSON.stringify(b)}`;
}

// ─── Target builders ──────────────────────────────────────────────────────────

const tmpBase = fs.mkdtempSync(path.join(os.tmpdir(), "ax-status-parity-"));

function copyDir(src, dst) {
  fs.mkdirSync(dst, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, entry.name);
    const d = path.join(dst, entry.name);
    if (entry.isDirectory()) {
      copyDir(s, d);
    } else {
      fs.copyFileSync(s, d);
    }
  }
}

/** (a) Repo root — installed, valid */
function targetA() {
  return repoRoot;
}

/** (b) Temp copy with an intentionally invalid doc (schema violation injected). */
function targetB() {
  const dir = path.join(tmpBase, "b-invalid-doc");
  const srcData = path.join(repoRoot, "docs", "architext", "data");
  const dstData = path.join(dir, "docs", "architext", "data");
  copyDir(repoRoot + "/docs/architext", dir + "/docs/architext");
  // Inject a schema violation: corrupt nodes.json by removing required "type"
  const nodesPath = path.join(dstData, "nodes.json");
  const nodes = JSON.parse(fs.readFileSync(nodesPath, "utf8"));
  if (nodes.nodes && nodes.nodes.length > 0) {
    delete nodes.nodes[0].type;
  }
  fs.writeFileSync(nodesPath, JSON.stringify(nodes, null, 2) + "\n");
  return dir;
}

/** (c) Temp dir with copied-install marker (legacy .architext-install.json present). */
function targetC() {
  const dir = path.join(tmpBase, "c-copied-install");
  const srcData = path.join(repoRoot, "docs", "architext", "data");
  const dstArchitextDir = path.join(dir, "docs", "architext");
  copyDir(repoRoot + "/docs/architext", dstArchitextDir);
  // Create legacy metadata marker
  const legacyMeta = path.join(dstArchitextDir, ".architext-install.json");
  fs.writeFileSync(legacyMeta, JSON.stringify({ schemaVersion: 1, installedAt: "2024-01-01T00:00:00.000Z" }, null, 2) + "\n");
  return dir;
}

/** (d) Temp non-installed dir (no docs/architext/data/manifest.json). */
function targetD() {
  const dir = path.join(tmpBase, "d-not-installed");
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

// ─── Main ─────────────────────────────────────────────────────────────────────

console.log(`Building Rust status_dump binary (first run may be slow)...`);
execFileSync("cargo", ["build", "-q", "-p", "architext-core", "--bin", "status_dump"], {
  cwd: repoRoot,
  stdio: "inherit"
});
console.log();

const targets = [
  { label: "(a) installed+valid (repo root)", build: targetA },
  { label: "(b) installed+invalid doc", build: targetB },
  { label: "(c) copied-install marker", build: targetC },
  { label: "(d) not-installed dir", build: targetD },
];

let allGreen = true;

for (const { label, build } of targets) {
  const targetDir = build();
  process.stdout.write(`${label} ... `);

  let oracle, candidate;
  try {
    oracle = runOracle(targetDir);
  } catch (e) {
    console.log(`RED (oracle error: ${e.message})`);
    allGreen = false;
    continue;
  }
  try {
    candidate = runCandidate(targetDir);
  } catch (e) {
    console.log(`RED (candidate error: ${e.message})`);
    allGreen = false;
    continue;
  }

  const normOracle = normalize(oracle);
  const normCandidate = normalize(candidate);
  const diff = firstDiff(normOracle, normCandidate);

  if (!diff) {
    console.log("GREEN");
  } else {
    console.log(`RED\n  First diff: ${diff}`);
    allGreen = false;
  }
}

// Clean up temp dirs
try { fs.rmSync(tmpBase, { recursive: true, force: true }); } catch {}

console.log();
if (allGreen) {
  console.log("All targets GREEN — status parity gate PASSED.");
} else {
  console.log("One or more targets RED — status parity gate FAILED.");
  process.exit(1);
}
