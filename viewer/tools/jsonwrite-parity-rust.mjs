#!/usr/bin/env node
/**
 * Differential gate: Rust write_json_string vs JS JSON.stringify(v, null, 2) + "\n"
 *
 * For every *.json under docs/architext/data/ AND every synthetic fixture
 * under crates/architext-core/tests/jsonwrite-fixtures/:
 *   1. Read raw bytes.
 *   2. Oracle: JSON.parse → JSON.stringify(v, null, 2) + "\n".
 *   3. Rust:   cargo run -q -p architext-core --bin jsonwrite_dump -- <file>
 *   4. Compare BYTE-FOR-BYTE.
 *
 * Output: per-input GREEN/RED + final summary line.
 * Exit: 0 if all GREEN, nonzero on any RED.
 */

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..", "..");

// ── collect files ────────────────────────────────────────────────────────────

async function collectJson(dir) {
  const results = [];
  async function walk(d) {
    let entries;
    try {
      entries = await readdir(d);
    } catch {
      return;
    }
    for (const name of entries) {
      const full = path.join(d, name);
      const s = await stat(full).catch(() => null);
      if (!s) continue;
      if (s.isDirectory()) {
        await walk(full);
      } else if (name.endsWith(".json")) {
        results.push(full);
      }
    }
  }
  await walk(dir);
  return results;
}

const dataDir = path.join(repoRoot, "docs", "architext", "data");
const fixtureDir = path.join(
  repoRoot,
  "crates",
  "architext-core",
  "tests",
  "jsonwrite-fixtures"
);

const [dataFiles, fixtureFiles] = await Promise.all([
  collectJson(dataDir),
  collectJson(fixtureDir),
]);

const allFiles = [
  ...dataFiles.map((f) => ({ file: f, kind: "data" })),
  ...fixtureFiles.map((f) => ({ file: f, kind: "fixture" })),
];

if (allFiles.length === 0) {
  console.error("No JSON files found. Check paths.");
  process.exit(1);
}

// ── run gate ─────────────────────────────────────────────────────────────────

let green = 0;
let red = 0;

function rustOutput(file) {
  return execFileSync(
    "cargo",
    ["run", "-q", "-p", "architext-core", "--bin", "jsonwrite_dump", "--", file],
    { cwd: repoRoot, encoding: "buffer" }
  );
}

function formatDiff(oracle, rust, maxContext = 80) {
  const oBytes = Buffer.from(oracle, "utf8");
  const rBytes = Buffer.from(rust, "utf8");
  const minLen = Math.min(oBytes.length, rBytes.length);
  let firstDiff = -1;
  for (let i = 0; i < minLen; i++) {
    if (oBytes[i] !== rBytes[i]) {
      firstDiff = i;
      break;
    }
  }
  if (firstDiff === -1 && oBytes.length !== rBytes.length) {
    firstDiff = minLen;
  }
  if (firstDiff === -1) return null; // identical

  const start = Math.max(0, firstDiff - 20);
  const end = Math.min(minLen, firstDiff + maxContext);
  const oSlice = JSON.stringify(oracle.slice(start, end));
  const rSlice = JSON.stringify(rust.slice(start, end));
  return [
    `  first diff at byte offset ${firstDiff}`,
    `  oracle[${start}..${end}]: ${oSlice}`,
    `  rust  [${start}..${end}]: ${rSlice}`,
    `  oracle length: ${oBytes.length}  rust length: ${rBytes.length}`,
  ].join("\n");
}

for (const { file, kind } of allFiles) {
  const raw = readFileSync(file, "utf8");
  const parsed = JSON.parse(raw);
  const oracle = JSON.stringify(parsed, null, 2) + "\n";

  let rustBuf;
  try {
    rustBuf = rustOutput(file);
  } catch (e) {
    console.error(`RED  [${kind}] ${path.relative(repoRoot, file)}`);
    console.error(`  cargo error: ${e.message}`);
    red++;
    continue;
  }

  const rustStr = rustBuf.toString("utf8");

  if (rustStr === oracle) {
    console.log(`GREEN [${kind}] ${path.relative(repoRoot, file)}`);
    green++;
  } else {
    console.error(`RED   [${kind}] ${path.relative(repoRoot, file)}`);
    const diff = formatDiff(oracle, rustStr);
    if (diff) console.error(diff);
    red++;
  }
}

// ── summary ──────────────────────────────────────────────────────────────────

const total = green + red;
console.log("");
console.log(
  `${green}/${total} GREEN  (${dataFiles.length} real data docs, ${fixtureFiles.length} synthetic fixtures)`
);

if (red > 0) {
  console.error(`FAILED: ${red} RED`);
  process.exit(1);
}
