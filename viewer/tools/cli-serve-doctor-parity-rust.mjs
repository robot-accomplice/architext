#!/usr/bin/env node
/**
 * CLI serve + doctor parity / behavior gate (CLI slice C).
 *
 * Compares the JS CLI (`tools/architext-adopt.mjs`) against the Rust CLI
 * (`target/debug/architext`) for the `doctor` command (differential parity),
 * exercises the `serve` lifecycle behaviorally (background → reachable → stop →
 * pid gone + state cleared), and unit-checks the pure helpers against the JS
 * expected values.
 *
 * Usage:  node viewer/tools/cli-serve-doctor-parity-rust.mjs
 * Exit 0 = all GREEN, nonzero = any RED.
 *
 * ───────────────────────────────────────────────────────────────────────────
 * NORMALISATIONS (doctor differential parity) — timestamps only, and the
 * documented validation-output divergence. Anything else is a hard RED.
 *
 *   1. ISO-8601 timestamps in regenerated files (releases/index.json
 *      `lastUpdated`) and in stdout are replaced with a fixed sentinel.
 *      RATIONALE: `new Date().toISOString()` (JS) / SystemTime (Rust) — cannot
 *      be made deterministic without mocking the clock. Every other byte of
 *      every regenerated file is compared verbatim.
 *   2. Absolute temp-dir paths in stdout → "<tmpdir>" (JS=/tmp, Rust path
 *      differ per OS; both print the resolved absolute target path).
 *   3. "CLI: <ver>" / version strings → "<version>" (JS reads package.json,
 *      Rust reads CARGO_PKG_VERSION = 0.0.0 during dev; same source of truth at
 *      release).
 *   4. `--json` ONLY: the `validation.output` string is dropped before
 *      comparing — the JS validator shells out and captures combined
 *      stdout/stderr, the Rust validator builds the text in-process; only
 *      `validation.ok` is load-bearing. This is the SAME documented
 *      normalisation already applied by status-parity-rust.mjs:86-89.
 *      `validation.output` IS compared verbatim in the non-JSON apply path
 *      because there both CLIs print the same Rust/JS validator wording line.
 *      (NOTE: in the non-JSON apply path the post-repair validation passes, so
 *      both print "Architext validation passed." — identical.)
 *
 * NO other normalisations. Any file-content or exit-code difference beyond the
 * above is reported RED and the gate fails.
 * ───────────────────────────────────────────────────────────────────────────
 */

import { spawnSync, spawn } from "node:child_process";
import {
  readFileSync, readdirSync, writeFileSync, mkdirSync, rmSync,
  mkdtempSync, cpSync, existsSync
} from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const jsEntry = path.join(repoRoot, "tools", "architext-adopt.mjs");
const srcDataDir = path.join(repoRoot, "docs", "architext", "data");

// ─── shell helpers ──────────────────────────────────────────────────────────

function run(cmd, args, cwd, env = {}) {
  const result = spawnSync(cmd, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, FORCE_COLOR: "0", NO_COLOR: "1", ...env },
  });
  return { stdout: result.stdout ?? "", stderr: result.stderr ?? "", status: result.status ?? 1 };
}

function buildRust() {
  process.stderr.write("Building Rust CLI (always rebuild)...\n");
  const result = spawnSync("cargo", ["build", "-q", "-p", "architext-cli"], { cwd: repoRoot, stdio: "inherit" });
  if (result.status !== 0) {
    console.error("cargo build failed");
    process.exit(1);
  }
}

function rustBin() {
  return path.join(repoRoot, "target", "debug", "architext");
}

// ─── result framework ───────────────────────────────────────────────────────

let passed = 0;
let failed = 0;
function record(name, ok, note = "") {
  if (ok) passed += 1; else failed += 1;
  const marker = ok ? "GREEN" : "RED  ";
  console.log(`${ok ? "✓" : "✗"} ${marker} ${name}${note ? `  (${note})` : ""}`);
}

// ─── doctor parity helpers ──────────────────────────────────────────────────

const TS_SENTINEL = "2000-01-01T00:00:00.000Z";
function normaliseTimestamps(text) {
  return text.replace(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/g, TS_SENTINEL);
}

function collectFiles(dir, base = dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) files.push(...collectFiles(full, base));
    else if (entry.isFile()) files.push(path.relative(base, full));
  }
  return files.sort();
}

/** Files whose content carries a regenerated timestamp; only these get TS-normalised. */
const TIMESTAMP_FILES = new Set([
  "docs/architext/data/releases/index.json",
]);

function normaliseFileContent(relPath, content) {
  let s = content;
  if (TIMESTAMP_FILES.has(relPath) || relPath.startsWith("docs/architext/data/releases/")) {
    s = normaliseTimestamps(s);
  }
  return s;
}

function normaliseStdout(text, tmpDir) {
  let s = text.replaceAll(tmpDir, "<tmpdir>");
  s = s.replace(/^CLI: \S+$/gm, "CLI: <version>");
  s = s.replace(/Architext CLI: \S+/g, "Architext CLI: <version>");
  s = normaliseTimestamps(s);
  return s;
}

/** Build a temp install (copy of repo data) with a DRIFTED release index so
 *  doctor detects a deterministic "regenerate Release Truth history" repair. */
function makeDriftedTarget(prefix) {
  const parent = mkdtempSync(path.join(tmpdir(), prefix));
  const target = path.join(parent, "my-project");
  const dd = path.join(target, "docs", "architext", "data");
  mkdirSync(dd, { recursive: true });
  cpSync(srcDataDir, dd, { recursive: true });

  const mani = JSON.parse(readFileSync(path.join(dd, "manifest.json"), "utf8"));
  const idxPath = path.join(dd, mani.files.releases);
  const idx = JSON.parse(readFileSync(idxPath, "utf8"));
  if (idx.releases && idx.releases[0]) {
    // Drift the first release's summary so the index disagrees with its detail
    // file → release-truth regeneration repair fires deterministically.
    idx.releases[0].summary = "STALE SUMMARY DRIFT — should be regenerated from detail";
  }
  writeFileSync(idxPath, JSON.stringify(idx, null, 2) + "\n");
  return { parent, target };
}

function jsDoctor(target, args) {
  return run(process.execPath, [jsEntry, "doctor", target, ...args], repoRoot);
}
function rustDoctor(target, args) {
  return run(rustBin(), ["doctor", target, ...args], repoRoot);
}

function indent(s) {
  return s.split("\n").map((l) => `    ${l}`).join("\n");
}

function compareFileTrees(label, tmpA, tmpB) {
  const failures = [];
  const aFiles = collectFiles(tmpA);
  const bFiles = collectFiles(tmpB);
  const aSet = new Set(aFiles), bSet = new Set(bFiles);
  for (const f of aFiles) if (!bSet.has(f)) failures.push(`only in JS: ${f}`);
  for (const f of bFiles) if (!aSet.has(f)) failures.push(`only in RS: ${f}`);
  for (const f of aFiles) {
    if (!bSet.has(f)) continue;
    const a = normaliseFileContent(f, readFileSync(path.join(tmpA, f), "utf8"));
    const b = normaliseFileContent(f, readFileSync(path.join(tmpB, f), "utf8"));
    if (a !== b) failures.push(`content differs: ${f}\n      JS:\n${indent(a).slice(0, 600)}\n      RS:\n${indent(b).slice(0, 600)}`);
  }
  return failures;
}

// ─── doctor: --yes apply (file tree byte-identical + exit) ──────────────────

{
  const A = makeDriftedTarget("ax-doc-js-");
  const B = makeDriftedTarget("ax-doc-rs-");
  try {
    const js = jsDoctor(A.target, ["--yes"]);
    const rs = rustDoctor(B.target, ["--yes"]);
    const failures = [];
    if (js.status !== rs.status) failures.push(`exit: JS=${js.status} RS=${rs.status}`);
    failures.push(...compareFileTrees("apply", A.target, B.target));
    // stdout: the "Applied doctor repairs:" + validation line should match
    // (timestamps + paths + version normalised).
    const jsOut = normaliseStdout(js.stdout, A.target);
    const rsOut = normaliseStdout(rs.stdout, B.target);
    if (jsOut !== rsOut) failures.push(`stdout mismatch:\n  JS:\n${indent(jsOut)}\n  RS:\n${indent(rsOut)}`);
    record("doctor --yes: regenerated file tree byte-identical + exit + stdout", failures.length === 0, failures.join(" | "));
  } finally {
    rmSync(A.parent, { recursive: true, force: true });
    rmSync(B.parent, { recursive: true, force: true });
  }
}

// ─── doctor: --dry-run (no writes; repair list matches; no file changes) ────

{
  const A = makeDriftedTarget("ax-docdry-js-");
  const B = makeDriftedTarget("ax-docdry-rs-");
  // Snapshot file trees before.
  const beforeA = collectFiles(A.target).map((f) => [f, readFileSync(path.join(A.target, f))]);
  const beforeB = collectFiles(B.target).map((f) => [f, readFileSync(path.join(B.target, f))]);
  try {
    const js = jsDoctor(A.target, ["--dry-run"]);
    const rs = rustDoctor(B.target, ["--dry-run"]);
    const failures = [];
    if (js.status !== rs.status) failures.push(`exit: JS=${js.status} RS=${rs.status}`);
    // No writes on either side.
    const noWrite = (before, target) =>
      before.every(([f, bytes]) => existsSync(path.join(target, f)) && readFileSync(path.join(target, f)).equals(bytes))
      && collectFiles(target).length === before.length;
    if (!noWrite(beforeA, A.target)) failures.push("JS dry-run wrote files");
    if (!noWrite(beforeB, B.target)) failures.push("RS dry-run wrote files");
    // Repair list: the "Doctor repairs available:" lines must match.
    const extractRepairs = (out) => {
      const lines = out.split("\n");
      const start = lines.indexOf("Doctor repairs available:");
      if (start < 0) return [];
      const repairs = [];
      for (let i = start + 1; i < lines.length && lines[i].startsWith("- "); i++) repairs.push(lines[i]);
      return repairs;
    };
    const jsR = extractRepairs(js.stdout), rsR = extractRepairs(rs.stdout);
    if (JSON.stringify(jsR) !== JSON.stringify(rsR)) failures.push(`repair list: JS=${JSON.stringify(jsR)} RS=${JSON.stringify(rsR)}`);
    if (jsR.length === 0) failures.push("expected at least one repair (drifted index)");
    // "Dry run: no doctor repairs applied." present on both.
    if (!js.stdout.includes("Dry run: no doctor repairs applied.")) failures.push("JS missing dry-run line");
    if (!rs.stdout.includes("Dry run: no doctor repairs applied.")) failures.push("RS missing dry-run line");
    record("doctor --dry-run: no writes + repair list matches", failures.length === 0, failures.join(" | "));
  } finally {
    rmSync(A.parent, { recursive: true, force: true });
    rmSync(B.parent, { recursive: true, force: true });
  }
}

// ─── doctor: --json (status object parity; validation.output normalised) ────

{
  const A = makeDriftedTarget("ax-docjson-js-");
  const B = makeDriftedTarget("ax-docjson-rs-");
  try {
    const js = jsDoctor(A.target, ["--json"]);
    const rs = rustDoctor(B.target, ["--json"]);
    const failures = [];
    if (js.status !== rs.status) failures.push(`exit: JS=${js.status} RS=${rs.status}`);
    let jsObj, rsObj;
    try { jsObj = JSON.parse(js.stdout); } catch (e) { failures.push(`JS json parse: ${e.message}`); }
    try { rsObj = JSON.parse(rs.stdout); } catch (e) { failures.push(`RS json parse: ${e.message}`); }
    if (jsObj && rsObj) {
      // Normalise: every absolute temp-dir path (target/dataDir AND nested
      // path-bearing strings like manifest.path, releaseTruth.indexPath) →
      // "<tmpdir>"; cliVersion → "<version>"; timestamps → sentinel; drop
      // validation.output (documented). The two temp dirs have different
      // basenames so every absolute path string legitimately differs.
      const norm = (o, tmp) => {
        const j = JSON.parse(JSON.stringify(o));
        const walk = (v) => {
          if (Array.isArray(v)) return v.map(walk);
          if (v && typeof v === "object") {
            const out = {};
            for (const [k, val] of Object.entries(v)) {
              if (k === "output") continue;                 // validation.output: documented drop
              if (k === "cliVersion") { out[k] = "<version>"; continue; }
              out[k] = walk(val);
            }
            return out;
          }
          if (typeof v === "string") return normaliseTimestamps(v.split(tmp).join("<tmpdir>"));
          return v;
        };
        return walk(j);
      };
      const nj = JSON.stringify(norm(jsObj, A.target), null, 2);
      const nr = JSON.stringify(norm(rsObj, B.target), null, 2);
      if (nj !== nr) {
        const njo = norm(jsObj, A.target), nro = norm(rsObj, B.target);
        const keys = new Set([...Object.keys(njo), ...Object.keys(nro)]);
        const diffKeys = [...keys].filter((k) => JSON.stringify(njo[k]) !== JSON.stringify(nro[k]));
        failures.push(`json status differs at keys: ${diffKeys.join(", ")}`);
      }
      // validation.ok must match (load-bearing).
      if (jsObj.validation?.ok !== rsObj.validation?.ok) failures.push(`validation.ok JS=${jsObj.validation?.ok} RS=${rsObj.validation?.ok}`);
    }
    record("doctor --json: status object parity (validation.output normalised)", failures.length === 0, failures.join(" | "));
  } finally {
    rmSync(A.parent, { recursive: true, force: true });
    rmSync(B.parent, { recursive: true, force: true });
  }
}

// ─── serve smoke (behavioral): background → reachable → stop → pid gone ─────

function findFreePortSync() {
  // Bind+close a server synchronously-ish via a 0-port; we just pick a likely
  // free port by binding and reading. Use a quick sync probe.
  const srv = net.createServer();
  return new Promise((resolve, reject) => {
    srv.once("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

async function waitFor(fn, timeoutMs, pollMs = 150) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (await fn()) return true;
    await new Promise((r) => setTimeout(r, pollMs));
  }
  return false;
}

function serveStateKey(target) {
  return createHash("sha256").update(path.resolve(target)).digest("hex").slice(0, 24);
}
function serveStatePath(target) {
  return path.join(tmpdir(), "architext-serve", `${serveStateKey(target)}.json`);
}

async function reachable(url) {
  try {
    const resp = await fetch(url, { method: "GET" });
    return resp.ok;
  } catch { return false; }
}

{
  // Behavioral serve smoke — the non-flaky path (background up + stop), with a
  // generous readiness poll and guaranteed cleanup of the spawned child.
  //
  // Data dir = the SMALL routing-corpus fixture, NOT the full repo data: the
  // server precomputes a plan farm at startup, and the repo's dense flows take
  // >>15s to enumerate (see memory: an 18-step flow alone is ~120s), which would
  // make the smoke test slow/flaky for reasons unrelated to the lifecycle. The
  // corpus farm builds in milliseconds, so readiness is a clean signal.
  const parent = mkdtempSync(path.join(tmpdir(), "ax-serve-rs-"));
  const target = path.join(parent, "my-project");
  const dd = path.join(target, "docs", "architext", "data");
  mkdirSync(dd, { recursive: true });
  cpSync(path.join(repoRoot, "test", "fixtures", "routing-corpus"), dd, { recursive: true });
  const distDir = path.join(repoRoot, "viewer", "dist");

  let everReachable = false, stateWritten = false, sessionOk = false;
  let stopCleared = false, pidGone = false;
  let backgroundPid = null;
  const env = { ARCHITEXT_VIEWER_DIST: distDir };
  const haveDist = existsSync(path.join(distDir, "index.html"));

  try {
    if (!haveDist) {
      record("serve smoke: background up → /api/session 200 → stop clears state + pid gone", false,
        "viewer/dist/index.html missing — run npm run build (serve smoke needs viewer assets)");
    } else {
      // Start in background on port 0 (auto-pick), no browser.
      const bg = run(rustBin(), ["serve", target, "--background", "--port", "0", "--no-open"], repoRoot, env);
      // Read state file to learn the URL + pid.
      const sp = serveStatePath(target);
      stateWritten = existsSync(sp);
      let url = null;
      if (stateWritten) {
        const state = JSON.parse(readFileSync(sp, "utf8"));
        url = state.url;
        backgroundPid = state.pid;
      }
      if (url) {
        everReachable = await waitFor(() => reachable(url), 10000);
        // /api/session must answer 200.
        try {
          const resp = await fetch(`${url}api/session`, { headers: { host: new URL(url).host } });
          sessionOk = resp.ok;
        } catch { sessionOk = false; }
      }

      // Stop via --stop (SIGKILL-safe escalation).
      run(rustBin(), ["serve", target, "--stop"], repoRoot, env);
      stopCleared = await waitFor(async () => !existsSync(sp), 5000);
      pidGone = backgroundPid ? await waitFor(async () => {
        try { process.kill(backgroundPid, 0); return false; } catch { return true; }
      }, 6000) : false;

      const ok = stateWritten && everReachable && sessionOk && stopCleared && pidGone;
      record("serve smoke: background up → /api/session 200 → stop clears state + pid gone", ok,
        ok ? "" : `stateWritten=${stateWritten} reachable=${everReachable} session200=${sessionOk} stopCleared=${stopCleared} pidGone=${pidGone}`);
    }
  } finally {
    // ALWAYS clean up: if anything left the child alive, kill it.
    if (backgroundPid) { try { process.kill(backgroundPid, "SIGKILL"); } catch {} }
    rmSync(parent, { recursive: true, force: true });
    try { rmSync(serveStatePath(target), { force: true }); } catch {}
  }
}

// ─── pure helpers (Rust unit-tested; here we re-assert the JS values) ───────
//
// The Rust unit tests (helpers.rs) assert is_loopback_serve_url /
// browser_open_command against these exact values. We mirror the JS truth here
// so the gate documents the cross-language contract in one place.

{
  // isLoopbackServeUrl truth (from serve-lifecycle.mjs).
  const cases = [
    ["http://127.0.0.1:4317/", true],
    ["http://localhost:4317/", true],
    ["http://[::1]:4317/", true],
    ["http://127.0.0.2/", true],
    ["http://0.0.0.0:4317/", false],
    ["https://127.0.0.1/", false],
    ["not a url", false],
  ];
  // We verify the JS implementation matches these (the Rust port is unit-tested
  // separately against the identical table).
  const { isLoopbackServeUrl } = await import(path.join(repoRoot, "src", "adapters", "cli", "serve-lifecycle.mjs"));
  const bad = cases.filter(([u, exp]) => isLoopbackServeUrl(u) !== exp);
  record("helper isLoopbackServeUrl: JS truth table (Rust unit-tested to match)", bad.length === 0,
    bad.map(([u, e]) => `${u} expected ${e}`).join(", "));

  const { browserOpenCommand } = await import(path.join(repoRoot, "src", "adapters", "cli", "serve-lifecycle.mjs"));
  const url = "http://x/";
  const expect = {
    darwin: { command: "open", args: [url] },
    win32: { command: "cmd", args: ["/c", "start", "", url] },
    linux: { command: "xdg-open", args: [url] },
  };
  const mismatches = [];
  for (const [plat, exp] of Object.entries(expect)) {
    const got = browserOpenCommand(plat, url);
    if (JSON.stringify(got) !== JSON.stringify(exp)) mismatches.push(`${plat}: ${JSON.stringify(got)}`);
  }
  if (browserOpenCommand("freebsd", url) !== null) mismatches.push("freebsd not null");
  record("helper browserOpenCommand: JS truth table (Rust unit-tested to match)", mismatches.length === 0, mismatches.join(", "));
}

// ─── summary ────────────────────────────────────────────────────────────────

console.log(`\ncli-serve-doctor parity: ${passed}/${passed + failed} GREEN`);
process.exit(failed === 0 ? 0 : 1);
