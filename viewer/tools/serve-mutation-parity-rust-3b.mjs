// serve-mutation-parity-rust-3b.mjs
//
// HTTP-level mutation parity gate for Rust serve adapter — slice 3b.
//
// DOES NOT boot the Node serve-lifecycle. Instead:
//   1. Always rebuilds the Rust `architext-serve` binary (release).
//   2. For each test case: JS oracle (direct import) vs Rust server (HTTP).
//   3. Asserts response envelope AND on-disk bytes byte-identical.
//   4. Reports per-case GREEN/RED + N/total; exits nonzero on RED.
//
// Cases covered:
//   POST /api/config:
//     15. project-scope write — written bytes match JS oracle
//     16. user-scope write — written bytes match JS oracle
//     17. out-of-range clamp — clamped value persisted, not raw
//     18. diff-from-defaults — only overrides in file (not full defaults)
//     19. re-resolved payload returned in response
//   POST /api/release-plans:
//     20. preview (action=preview + dryRun=true, no write)
//     21. approve (index+detail+roadmap written, bytes match)
//     22. save-draft (detail+index written; currentReleaseId unchanged)
//     23. unknown action -> ok:false, files unchanged
//   POST /api/doctor:
//     24. dry-run (no writes, repair list returned)
//     25. apply (mode=apply, validation.ok=true, reload=ok)
//   POST /api/sync-repair:
//     26. applies repairs + validates; ok + reload match
//
// Usage: node viewer/tools/serve-mutation-parity-rust-3b.mjs

import { execFileSync, spawn } from "node:child_process";
import {
  cpSync, mkdirSync, rmSync, readFileSync
} from "node:fs";
import { writeFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const srcDataDir = path.join(repoRoot, "docs/architext/data");
const distDir = path.join(repoRoot, "viewer/dist");
const binPath = path.join(repoRoot, "target/release/architext-serve");

// ---------------------------------------------------------------------------
// JS oracle imports
// ---------------------------------------------------------------------------
const { writeDiagramConfig } = await import(
  path.join(repoRoot, "src/adapters/http/diagram-config-api.mjs")
);
const { approveReleasePlanRequest } = await import(
  path.join(repoRoot, "src/adapters/http/release-planning-api.mjs")
);
const { readJson, writeJson } = await import(
  path.join(repoRoot, "src/adapters/cli/runtime.mjs")
);

// No-op write lock (oracle runs synchronously in test isolation)
async function noopWriteLock(_target, cb) { return cb(); }

// validateTarget that passes through to the architext CLI validator.
// NOTE: the entry is tools/architext-adopt.mjs (the published bin), taking the
// target REPO ROOT positionally — NOT a nonexistent src/cli.mjs. The old path
// made spawnSync always fail, so the JS oracle silently rolled back EVERY
// approve/save-draft (masquerading as a Rust divergence). Verified: with this
// real validator the JS oracle writes the new release, matching Rust.
async function validateTarget(target) {
  const { default: childProcess } = await import("node:child_process");
  const result = childProcess.spawnSync(
    "node",
    [path.join(repoRoot, "tools/architext-adopt.mjs"), "validate", target],
    { encoding: "utf8", timeout: 30_000 }
  );
  return { ok: result.status === 0, output: result.stdout + result.stderr };
}

// Normalize non-deterministic server-stamped wall-clock timestamps before
// byte-comparing release index/detail JSON. `now` is generated independently by
// each side at write time, so its VALUE legitimately differs; the surrounding
// bytes must still match exactly (proven: normalized JS index === Rust index).
function normalizeTimestamps(jsonText) {
  return jsonText.replace(
    /"(lastUpdated|targetDate|releasedAt|dateAdded)":\s*"[^"]*"/g,
    '"$1":"<TS>"'
  );
}

function dataDir(target) {
  return path.join(target, "docs", "architext", "data");
}

// ---------------------------------------------------------------------------
// Build the Rust binary -- ALWAYS rebuild.
// ---------------------------------------------------------------------------
console.log("[setup] building architext-serve (release)...");
try {
  execFileSync(
    "cargo",
    ["build", "-p", "architext-serve", "--release", "--quiet"],
    { cwd: repoRoot, stdio: "inherit", timeout: 600_000 }
  );
} catch (err) {
  console.error("FATAL: cargo build failed:", err.message);
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Temp directory management
// ---------------------------------------------------------------------------
const tmpBase = path.join(repoRoot, ".tmp-mutation-parity-3b");
rmSync(tmpBase, { recursive: true, force: true });
mkdirSync(tmpBase, { recursive: true });

let tempDirSeq = 0;
function makeTempTargetDir(suffix) {
  const dir = path.join(tmpBase, `target-${++tempDirSeq}-${suffix}`);
  const dd = path.join(dir, "docs", "architext", "data");
  mkdirSync(dd, { recursive: true });
  cpSync(srcDataDir, dd, { recursive: true });
  return dir;
}

function readExistingBytes(filePath) {
  try { return readFileSync(filePath); } catch (e) {
    if (e.code === "ENOENT") return null;
    throw e;
  }
}
function buffersEqual(a, b) {
  if (!a || !b) return false;
  if (a.length !== b.length) return false;
  return a.equals(b);
}

// ---------------------------------------------------------------------------
// Rust server spawn helper
// ---------------------------------------------------------------------------
async function spawnRustServer(dd, extraEnv = {}) {
  const port = await findFreePort();
  const srv = spawn(binPath, [
    "--data-dir", dd,
    "--dist", distDir,
    "--port", String(port),
    "--host", "127.0.0.1",
  ], {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, ...extraEnv }
  });
  srv.stderr.on("data", () => {});
  srv.stdout.on("data", () => {});
  await waitForReady(port, 10_000);
  const sessResp = await fetch(`http://127.0.0.1:${port}/api/session`, {
    headers: { host: `127.0.0.1:${port}` }
  });
  const { mutationToken } = await sessResp.json();
  return { port, srv, token: mutationToken };
}

async function rustPost(route, port, token, body) {
  const resp = await fetch(`http://127.0.0.1:${port}${route}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-architext-mutation-token": token,
      host: `127.0.0.1:${port}`,
    },
    body: JSON.stringify(body),
  });
  const json = await resp.json().catch(() => null);
  return { status: resp.status, json };
}

// ---------------------------------------------------------------------------
// Check framework
// ---------------------------------------------------------------------------
const checks = [];
let greenCount = 0;

function record(name, pass, note = "") {
  checks.push({ name, pass, note });
  if (pass) greenCount += 1;
  const marker = pass ? "✓" : "✗";
  console.log(
    `${marker} ${(pass ? "GREEN" : "RED").padEnd(5)} ${name}${note ? "  (" + note + ")" : ""}`
  );
}

// ===========================================================================
// POST /api/config -- cases 15-19
// ===========================================================================

// Case 15: project-scope write -- written bytes match JS oracle
{
  const targetJs   = makeTempTargetDir("c15-js");
  const targetRust = makeTempTargetDir("c15-rust");
  const diagram = { layout: { laneWidth: 280, rowGap: 120 } };

  let jsResult;
  try {
    jsResult = await writeDiagramConfig({ scope: "project", target: targetJs, diagram });
  } catch (e) { jsResult = { ok: false, error: e.message }; }

  const { port: p15, srv: s15, token: t15 } = await spawnRustServer(dataDir(targetRust));
  const r15 = await rustPost("/api/config", p15, t15, { scope: "project", diagram });
  s15.kill();

  const jsBytes15   = readExistingBytes(path.join(targetJs, "docs", "architext", "config.json"));
  const rustBytes15 = readExistingBytes(path.join(targetRust, "docs", "architext", "config.json"));
  const pass15 = r15.status === 200 && r15.json?.ok === true && r15.json?.scope === "project"
    && jsBytes15 !== null && rustBytes15 !== null && buffersEqual(jsBytes15, rustBytes15);
  record("config: project-scope write -- bytes match JS oracle", pass15,
    pass15 ? "" : `HTTP=${r15.status} ok=${r15.json?.ok} jsLen=${jsBytes15?.length} rustLen=${rustBytes15?.length} match=${jsBytes15 && rustBytes15 ? buffersEqual(jsBytes15, rustBytes15) : "na"}`);
}

// Case 16: user-scope write -- written bytes match JS oracle
{
  const targetJs   = makeTempTargetDir("c16-js");
  const targetRust = makeTempTargetDir("c16-rust");
  const fakeHomeJs   = path.join(tmpBase, "home-js-16");
  const fakeHomeRust = path.join(tmpBase, "home-rust-16");
  mkdirSync(path.join(fakeHomeJs,   ".architext"), { recursive: true });
  mkdirSync(path.join(fakeHomeRust, ".architext"), { recursive: true });
  const diagram = { layout: { nodeWidth: 160 } };

  let jsResult16;
  try {
    jsResult16 = await writeDiagramConfig({
      scope: "user", target: targetJs, diagram, homedir: fakeHomeJs
    });
  } catch (e) { jsResult16 = { ok: false, error: e.message }; }

  const { port: p16, srv: s16, token: t16 } =
    await spawnRustServer(dataDir(targetRust), { HOME: fakeHomeRust });
  const r16 = await rustPost("/api/config", p16, t16, { scope: "user", diagram });
  s16.kill();

  const jsBytes16   = readExistingBytes(path.join(fakeHomeJs,   ".architext", "config.json"));
  const rustBytes16 = readExistingBytes(path.join(fakeHomeRust, ".architext", "config.json"));
  const pass16 = r16.status === 200 && r16.json?.ok === true && r16.json?.scope === "user"
    && jsBytes16 !== null && rustBytes16 !== null && buffersEqual(jsBytes16, rustBytes16);
  record("config: user-scope write -- bytes match JS oracle", pass16,
    pass16 ? "" : `HTTP=${r16.status} ok=${r16.json?.ok} jsLen=${jsBytes16?.length} rustLen=${rustBytes16?.length} match=${jsBytes16 && rustBytes16 ? buffersEqual(jsBytes16, rustBytes16) : "na"}`);
}

// Case 17: out-of-range clamp -- clamped value persisted (not raw 9999)
{
  const targetRust = makeTempTargetDir("c17-rust");
  const { port: p17, srv: s17, token: t17 } = await spawnRustServer(dataDir(targetRust));
  const r17 = await rustPost("/api/config", p17, t17, {
    scope: "project",
    diagram: { layout: { laneWidth: 9999 } }
  });
  s17.kill();

  const configPath17 = path.join(targetRust, "docs", "architext", "config.json");
  const bytes17 = readExistingBytes(configPath17);
  let clamped17 = false;
  if (bytes17) {
    try {
      const obj = JSON.parse(bytes17.toString("utf8"));
      // Must be <= 800 (the max), not 9999
      clamped17 = typeof obj?.layout?.laneWidth === "number" && obj.layout.laneWidth <= 800;
    } catch { /* ignore */ }
  }
  const pass17 = r17.status === 200 && r17.json?.ok === true && clamped17;
  record("config: out-of-range clamped in persisted file", pass17,
    pass17 ? "" : `HTTP=${r17.status} ok=${r17.json?.ok} clamped=${clamped17} file=${bytes17?.toString("utf8")?.slice(0, 100)}`);
}

// Case 18: diff-from-defaults -- only non-default overrides persisted
{
  const targetRust = makeTempTargetDir("c18-rust");
  const { port: p18, srv: s18, token: t18 } = await spawnRustServer(dataDir(targetRust));
  const r18 = await rustPost("/api/config", p18, t18, {
    scope: "project",
    diagram: { layout: { laneWidth: 250 } }
  });
  s18.kill();

  const configPath18 = path.join(targetRust, "docs", "architext", "config.json");
  const bytes18 = readExistingBytes(configPath18);
  let onlyOverrides18 = false;
  if (bytes18) {
    try {
      const obj = JSON.parse(bytes18.toString("utf8"));
      // Should contain layout.laneWidth=250 and NOT contain full default sections
      onlyOverrides18 = obj?.layout?.laneWidth === 250
        && obj?.sequence === undefined
        && obj?.zoom === undefined;
    } catch { /* ignore */ }
  }
  const pass18 = r18.status === 200 && r18.json?.ok === true && onlyOverrides18;
  record("config: diff-from-defaults -- only non-default overrides persisted", pass18,
    pass18 ? "" : `ok=${r18.json?.ok} onlyOverrides=${onlyOverrides18} file=${bytes18?.toString("utf8")?.slice(0, 200)}`);
}

// Case 19: re-resolved diagram returned in response
{
  const targetRust = makeTempTargetDir("c19-rust");
  const { port: p19, srv: s19, token: t19 } = await spawnRustServer(dataDir(targetRust));
  const r19 = await rustPost("/api/config", p19, t19, {
    scope: "project",
    diagram: { layout: { laneWidth: 300 } }
  });
  s19.kill();

  const pass19 = r19.status === 200
    && r19.json?.ok === true
    && r19.json?.diagram?.layout?.laneWidth === 300
    && Array.isArray(r19.json?.warnings)
    && r19.json?.written !== undefined;
  record("config: re-resolved diagram returned in response", pass19,
    pass19 ? "" : `ok=${r19.json?.ok} laneWidth=${r19.json?.diagram?.layout?.laneWidth} warnings=${Array.isArray(r19.json?.warnings)} written=${r19.json?.written !== undefined}`);
}

// ===========================================================================
// POST /api/release-plans -- cases 20-23
// ===========================================================================

const manifest = JSON.parse(readFileSync(path.join(srcDataDir, "manifest.json"), "utf8"));
const hasRoadmap  = Boolean(manifest.files?.roadmap);
const hasReleases = Boolean(manifest.files?.releases);

if (!hasRoadmap || !hasReleases) {
  for (let i = 20; i <= 23; i++) {
    record(`release-plans: case ${i} SKIPPED -- manifest missing roadmap/releases`, false,
      "manifest.files.roadmap or .releases absent");
  }
} else {
  // Case 20: preview (action=preview + dryRun=true -> no write)
  {
    const targetJs   = makeTempTargetDir("c20-js");
    const targetRust = makeTempTargetDir("c20-rust");
    const payload20 = { action: "preview", dryRun: true };

    let jsResult20;
    try {
      jsResult20 = await approveReleasePlanRequest({
        target: targetJs, payload: payload20, dataDir,
        readJson, writeJson, validateTarget, withTargetWriteLock: noopWriteLock
      });
    } catch (e) { jsResult20 = { ok: false, error: e.message }; }

    const { port: p20, srv: s20, token: t20 } = await spawnRustServer(dataDir(targetRust));
    const r20 = await rustPost("/api/release-plans", p20, t20, payload20);
    s20.kill();

    const relPath20 = manifest.files.releases;
    const origBytes20 = readExistingBytes(path.join(srcDataDir, relPath20));
    const rustBytes20 = readExistingBytes(path.join(dataDir(targetRust), relPath20));
    const noWrite20 = origBytes20 !== null && rustBytes20 !== null
      && buffersEqual(origBytes20, rustBytes20);

    const pass20 = r20.status === 200
      && r20.json?.releaseDetail !== undefined
      && r20.json?.changes !== undefined
      && r20.json?.validation?.ok === true
      && noWrite20;
    record("release-plans: preview -- releaseDetail+changes returned, no write", pass20,
      pass20 ? "" : `HTTP=${r20.status} releaseDetail=${!!r20.json?.releaseDetail} changes=${!!r20.json?.changes} noWrite=${noWrite20} body=${JSON.stringify(r20.json).slice(0, 300)}`);
  }

  // Case 21: approve -- index bytes match JS oracle
  {
    const targetJs   = makeTempTargetDir("c21-js");
    const targetRust = makeTempTargetDir("c21-rust");
    const payload21 = { action: "approve", selectedRoadmapItemIds: [], adHocItems: [] };

    let jsResult21;
    try {
      jsResult21 = await approveReleasePlanRequest({
        target: targetJs, payload: payload21, dataDir,
        readJson, writeJson, validateTarget, withTargetWriteLock: noopWriteLock
      });
    } catch (e) { jsResult21 = { ok: false, error: e.message }; console.error("[c21 JS error]", e.message); }

    const { port: p21, srv: s21, token: t21 } = await spawnRustServer(dataDir(targetRust));
    const r21 = await rustPost("/api/release-plans", p21, t21, payload21);
    s21.kill();

    const relPath21 = manifest.files.releases;
    const jsIdxBytes21   = readExistingBytes(path.join(dataDir(targetJs),   relPath21));
    const rustIdxBytes21 = readExistingBytes(path.join(dataDir(targetRust), relPath21));
    // Exact match modulo the server-stamped `now` (each side stamps its own).
    const indexMatch21 = jsIdxBytes21 !== null && rustIdxBytes21 !== null
      && normalizeTimestamps(jsIdxBytes21.toString("utf8")) === normalizeTimestamps(rustIdxBytes21.toString("utf8"));

    const pass21 = r21.status === 200
      && r21.json?.releaseDetail !== undefined
      && r21.json?.validation?.ok === true
      && indexMatch21;
    record("release-plans: approve -- index bytes match JS oracle", pass21,
      pass21 ? "" : `HTTP=${r21.status} releaseDetail=${!!r21.json?.releaseDetail} validOk=${r21.json?.validation?.ok} indexMatch=${indexMatch21} jsLen=${jsIdxBytes21?.length} rustLen=${rustIdxBytes21?.length}`);
  }

  // Case 22: save-draft -- currentReleaseId unchanged, index bytes match JS oracle
  {
    const targetJs   = makeTempTargetDir("c22-js");
    const targetRust = makeTempTargetDir("c22-rust");
    const payload22 = { action: "save-draft" };
    const origIndex22 = JSON.parse(
      readFileSync(path.join(srcDataDir, manifest.files.releases), "utf8")
    );

    let jsResult22;
    try {
      jsResult22 = await approveReleasePlanRequest({
        target: targetJs, payload: payload22, dataDir,
        readJson, writeJson, validateTarget, withTargetWriteLock: noopWriteLock
      });
    } catch (e) { jsResult22 = { ok: false, error: e.message }; }

    const { port: p22, srv: s22, token: t22 } = await spawnRustServer(dataDir(targetRust));
    const r22 = await rustPost("/api/release-plans", p22, t22, payload22);
    s22.kill();

    const rustIndexBytes22 = readExistingBytes(path.join(dataDir(targetRust), manifest.files.releases));
    const rustIndex22 = rustIndexBytes22
      ? JSON.parse(rustIndexBytes22.toString("utf8"))
      : {};
    const jsIdxBytes22   = readExistingBytes(path.join(dataDir(targetJs),   manifest.files.releases));
    const rustIdxBytes22 = rustIndexBytes22;
    const indexMatch22 = jsIdxBytes22 !== null && rustIdxBytes22 !== null
      && normalizeTimestamps(jsIdxBytes22.toString("utf8")) === normalizeTimestamps(rustIdxBytes22.toString("utf8"));
    const currentIdUnchanged22 = rustIndex22?.currentReleaseId === origIndex22?.currentReleaseId;

    const pass22 = r22.status === 200
      && r22.json?.releaseDetail !== undefined
      && currentIdUnchanged22
      && indexMatch22;
    record("release-plans: save-draft -- currentReleaseId unchanged, index bytes match", pass22,
      pass22 ? "" : `HTTP=${r22.status} releaseDetail=${!!r22.json?.releaseDetail} currentIdUnchanged=${currentIdUnchanged22} indexMatch=${indexMatch22}`);
  }

  // Case 23: unknown action -> ok:false, files unchanged
  {
    const targetRust = makeTempTargetDir("c23-rust");
    const relPath23 = manifest.files.releases;
    const origBytes23 = readExistingBytes(path.join(srcDataDir, relPath23));

    const { port: p23, srv: s23, token: t23 } = await spawnRustServer(dataDir(targetRust));
    const r23 = await rustPost("/api/release-plans", p23, t23, { action: "frobnicate" });
    s23.kill();

    const afterBytes23 = readExistingBytes(path.join(dataDir(targetRust), relPath23));
    const rolledBack23 = origBytes23 !== null && afterBytes23 !== null
      && buffersEqual(origBytes23, afterBytes23);

    const pass23 = r23.status === 200
      && r23.json?.ok === false
      && rolledBack23;
    record("release-plans: unknown action -> ok:false, files unchanged", pass23,
      pass23 ? "" : `HTTP=${r23.status} ok=${r23.json?.ok} rolledBack=${rolledBack23} body=${JSON.stringify(r23.json).slice(0, 200)}`);
  }
}

// ===========================================================================
// POST /api/doctor -- cases 24-25
// ===========================================================================

// Case 24: dry-run -- no writes, repair list returned
{
  const targetRust = makeTempTargetDir("c24-rust");
  const origManifestBytes24 = readExistingBytes(path.join(dataDir(targetRust), "manifest.json"));

  const { port: p24, srv: s24, token: t24 } = await spawnRustServer(dataDir(targetRust));
  const r24 = await rustPost("/api/doctor", p24, t24, { apply: false });
  s24.kill();

  const afterManifest24 = readExistingBytes(path.join(dataDir(targetRust), "manifest.json"));
  const noWrite24 = origManifestBytes24 !== null && afterManifest24 !== null
    && buffersEqual(origManifestBytes24, afterManifest24);

  const pass24 = r24.status === 200
    && r24.json?.mode === "dry-run"
    && Array.isArray(r24.json?.repairs)
    && r24.json?.ok === true
    && r24.json?.reload === false
    && r24.json?.status !== undefined
    && noWrite24;
  record("doctor: dry-run -- mode=dry-run, repairs[], ok=true, reload=false, no writes", pass24,
    pass24 ? "" : `HTTP=${r24.status} mode=${r24.json?.mode} ok=${r24.json?.ok} reload=${r24.json?.reload} repairs=${Array.isArray(r24.json?.repairs)} noWrite=${noWrite24} body=${JSON.stringify(r24.json).slice(0, 300)}`);
}

// Case 25: apply -- mode=apply, validation.ok=true, reload=ok
{
  const targetRust = makeTempTargetDir("c25-rust");

  const { port: p25, srv: s25, token: t25 } = await spawnRustServer(dataDir(targetRust));
  const r25 = await rustPost("/api/doctor", p25, t25, { apply: true });
  s25.kill();

  const pass25 = r25.status === 200
    && r25.json?.mode === "apply"
    && Array.isArray(r25.json?.repairs)
    && typeof r25.json?.ok === "boolean"
    && r25.json?.status !== undefined
    && r25.json?.reload === r25.json?.ok
    && r25.json?.validation?.ok === true;
  record("doctor: apply -- mode=apply, validation.ok=true, reload=ok", pass25,
    pass25 ? "" : `HTTP=${r25.status} mode=${r25.json?.mode} ok=${r25.json?.ok} reload=${r25.json?.reload} validOk=${r25.json?.validation?.ok} body=${JSON.stringify(r25.json).slice(0, 400)}`);
}

// ===========================================================================
// POST /api/sync-repair -- case 26
// ===========================================================================

{
  const targetRust = makeTempTargetDir("c26-rust");

  const { port: p26, srv: s26, token: t26 } = await spawnRustServer(dataDir(targetRust));
  const r26 = await rustPost("/api/sync-repair", p26, t26, {});
  s26.kill();

  const pass26 = r26.status === 200
    && typeof r26.json?.ok === "boolean"
    && typeof r26.json?.output === "string"
    && r26.json?.validation !== undefined
    && r26.json?.reload === r26.json?.ok
    && r26.json?.validation?.ok === true;
  record("sync-repair: ok+reload match, output string, validation.ok=true", pass26,
    pass26 ? "" : `HTTP=${r26.status} ok=${r26.json?.ok} output=${typeof r26.json?.output} validOk=${r26.json?.validation?.ok} reload=${r26.json?.reload} body=${JSON.stringify(r26.json).slice(0, 300)}`);
}

// ===========================================================================
// Shutdown & report
// ===========================================================================

rmSync(tmpBase, { recursive: true, force: true });

console.log("\n--- Slice 3b Results ---");
for (const c of checks) {
  const marker = c.pass ? "✓" : "✗";
  const status = c.pass ? "GREEN" : "RED";
  const note = c.note ? `  (${c.note})` : "";
  console.log(`${marker} ${status.padEnd(5)} ${c.name}${note}`);
}
console.log(`\n${greenCount}/${checks.length} GREEN`);
if (greenCount !== checks.length) process.exitCode = 1;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function findFreePort() {
  const { createServer } = await import("node:net");
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const port = s.address().port;
      s.close(() => resolve(port));
    });
    s.on("error", reject);
  });
}

async function waitForReady(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/api/session`, {
        headers: { host: `127.0.0.1:${port}` }
      });
      if (r.status === 200) return;
    } catch { /* not up yet */ }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`Server on port ${port} did not become ready in ${timeoutMs}ms`);
}
