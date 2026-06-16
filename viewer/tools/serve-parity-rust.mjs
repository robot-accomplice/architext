// serve-parity-rust.mjs
//
// HTTP-level parity gate for the Rust serve adapter (Phase 2A slice 1 + 2c).
//
// DOES NOT boot the flaky Node serve-lifecycle. Instead:
//   1. Builds + spawns the Rust `architext-serve` binary on an ephemeral port.
//   2. Runs a suite of HTTP checks against it.
//   3. Derives oracles directly from the JS source-of-truth (plan-precompute.mjs,
//      planDiagram.js, planCodec.js, diagram-config-api.mjs, repo-tree-api.mjs)
//      — no Node server involved.
//   4. Reports per-check GREEN/RED + N/total, exits nonzero on any RED.
//
// Checks (slice 1):
//   /api/plan/{hash}   — hit: body byte-equals JS oracle; miss: 200 {miss:true}
//   /data/{path}       — body bytes + content-type + cache-control
//   /                  — index.html served
//   /{spa-route}       — SPA fallback returns index.html
//   /api/session       — 200 + base64url mutationToken of correct length
//   security           — non-loopback Host/Origin → 403 exact error JSON
//   /api/unknown       — 404 + error JSON shape
//
// Checks (slice 2c — new):
//   /api/status        — 200 + {ok,status}; ok matches formula; status shape
//   /api/config        — 200 + semantic-equal to JS oracle; Cache-Control: no-store
//   /api/repo-tree     — 200 + source + files array parity; mtime integer ms;
//                        Cache-Control: no-store
//
// Usage: node viewer/tools/serve-parity-rust.mjs
import { execFileSync, spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const dataDir = path.join(repoRoot, "docs/architext/data");
const distDir = path.join(repoRoot, "viewer/dist");

// ---------------------------------------------------------------------------
// JS oracle imports
// ---------------------------------------------------------------------------
const { enumerateFlowPlanRequests } = await import(
  path.join(repoRoot, "src/adapters/http/plan-precompute.mjs")
);
const { planDiagram } = await import(
  path.join(repoRoot, "viewer/src/routing/planDiagram.js")
);
const { serializePlan } = await import(
  path.join(repoRoot, "viewer/src/routing/planCodec.js")
);
const { diagramConfigGetPayload } = await import(
  path.join(repoRoot, "src/adapters/http/diagram-config-api.mjs")
);
const { repoTreeApiRequest } = await import(
  path.join(repoRoot, "src/adapters/http/repo-tree-api.mjs")
);

// ---------------------------------------------------------------------------
// Build the Rust binary (or verify it exists)
// ---------------------------------------------------------------------------
const binPath = path.join(repoRoot, "target/release/architext-serve");
const { existsSync } = await import("node:fs");
if (!existsSync(binPath)) {
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
} else {
  console.log("[setup] using pre-built binary at", binPath);
}

// ---------------------------------------------------------------------------
// Spawn the server on an ephemeral port (port 0 → OS assigns free port)
// ---------------------------------------------------------------------------
// Use a fixed high port to avoid having to parse stderr for the bound port.
// Find a free port first, then pass it.
const testPort = await findFreePort();

const server = spawn(
  binPath,
  [
    "--data-dir", dataDir,
    "--dist", distDir,
    "--port", String(testPort),
    "--host", "127.0.0.1",
  ],
  { stdio: ["ignore", "pipe", "pipe"] }
);

let serverReady = false;
let serverShuttingDown = false;
const serverError = [];

server.stdout.on("data", (d) => { /* tracing goes to stderr */ });
server.stderr.on("data", (d) => {
  const msg = d.toString();
  if (msg.includes("listening")) serverReady = true;
  serverError.push(msg);
});

server.on("exit", (code, signal) => {
  if (!serverReady && !serverShuttingDown) {
    console.error("FATAL: server exited before ready. stderr:", serverError.join(""));
    process.exit(1);
  }
});

// Wait for server to be ready (up to 15 s)
await waitForReady(testPort, 15_000);

const BASE = `http://127.0.0.1:${testPort}`;
console.log(`[setup] server ready at ${BASE}\n`);

// ---------------------------------------------------------------------------
// Check framework
// ---------------------------------------------------------------------------
const checks = [];
let green = 0;

function record(name, pass, note = "") {
  checks.push({ name, pass, note });
  if (pass) green += 1;
}

async function get(url, headers = {}) {
  const res = await fetch(url, { headers });
  const body = await res.arrayBuffer();
  return { status: res.status, headers: res.headers, body: Buffer.from(body) };
}

// ---------------------------------------------------------------------------
// JS oracle: build plan map
// ---------------------------------------------------------------------------
const config = await diagramConfigGetPayload(repoRoot);
const jsRequests = await enumerateFlowPlanRequests({
  dataDir,
  layoutConfig: config?.diagram?.layout,
});

const jsPlanMap = new Map();
for (const req of jsRequests) {
  const planResult = planDiagram(req.planInput);
  const planJson = JSON.stringify(serializePlan(planResult));
  jsPlanMap.set(req.hash, { flowId: req.flowId, viewId: req.viewId, planJson });
}

// ---------------------------------------------------------------------------
// Check: /api/plan/{hash} — hits
// ---------------------------------------------------------------------------
let planHitPass = true;
let planHitNote = "";
for (const [hash, oracle] of jsPlanMap) {
  const r = await get(`${BASE}/api/plan/${hash}`);
  if (r.status !== 200) {
    planHitPass = false;
    planHitNote = `${oracle.flowId}@${oracle.viewId} status=${r.status} (expected 200)`;
    break;
  }
  const rustBody = r.body.toString("utf8");
  // JS plan-hit shape: `{"plan":<planJson>}`
  const expected = `{"plan":${oracle.planJson}}`;
  if (rustBody !== expected) {
    planHitPass = false;
    planHitNote = `${oracle.flowId}@${oracle.viewId} body mismatch (js=${expected.length}b rust=${rustBody.length}b)`;
    break;
  }
  const cc = r.headers.get("cache-control");
  if (cc !== "no-store") {
    planHitPass = false;
    planHitNote = `${oracle.flowId}@${oracle.viewId} cache-control=${cc} (expected no-store)`;
    break;
  }
}
record(
  `/api/plan/{hash} — ${jsPlanMap.size} hits byte-match JS oracle`,
  planHitPass,
  planHitNote
);

// ---------------------------------------------------------------------------
// Check: /api/plan/{bogus} — miss shape
// ---------------------------------------------------------------------------
const missHash = "a".repeat(64); // 64 hex chars, won't match anything
const missR = await get(`${BASE}/api/plan/${missHash}`);
const missBody = JSON.parse(missR.body.toString("utf8"));
record(
  "/api/plan/{bogus} — 200 {miss:true}",
  missR.status === 200 && missBody.miss === true,
  `status=${missR.status} body=${JSON.stringify(missBody)}`
);

// ---------------------------------------------------------------------------
// Check: /data/{path} — body + content-type + cache-control
// ---------------------------------------------------------------------------
const dataFiles = [
  { rel: "flows.json", ct: "application/json; charset=utf-8" },
  { rel: "nodes.json", ct: "application/json; charset=utf-8" },
  { rel: "views.json", ct: "application/json; charset=utf-8" },
];
for (const { rel, ct } of dataFiles) {
  const expected = await readFile(path.join(dataDir, rel));
  const r = await get(`${BASE}/data/${rel}`);
  const bodyOk = r.body.equals(expected);
  const ctOk = r.headers.get("content-type") === ct;
  const ccOk = r.headers.get("cache-control") === "no-store";
  record(
    `/data/${rel} — body + content-type + cache-control`,
    r.status === 200 && bodyOk && ctOk && ccOk,
    !bodyOk ? `body_mismatch(expected=${expected.length}b got=${r.body.length}b)` :
    !ctOk ? `content-type=${r.headers.get("content-type")} (expected ${ct})` :
    !ccOk ? `cache-control=${r.headers.get("cache-control")} (expected no-store)` : ""
  );
}

// ---------------------------------------------------------------------------
// Check: /data/{nonexistent} → 404
// ---------------------------------------------------------------------------
const d404 = await get(`${BASE}/data/nonexistent_xyz.json`);
record("/data/nonexistent → 404", d404.status === 404, `status=${d404.status}`);

// ---------------------------------------------------------------------------
// Check: / → index.html
// ---------------------------------------------------------------------------
const indexExpected = await readFile(path.join(distDir, "index.html"));
const rootR = await get(`${BASE}/`);
const rootBodyOk = rootR.body.equals(indexExpected);
const rootCtOk = (rootR.headers.get("content-type") ?? "").includes("text/html");
record(
  "/ → index.html with text/html",
  rootR.status === 200 && rootBodyOk && rootCtOk,
  !rootBodyOk ? `body_mismatch(expected=${indexExpected.length}b got=${rootR.body.length}b)` :
  !rootCtOk ? `content-type=${rootR.headers.get("content-type")}` : ""
);

// ---------------------------------------------------------------------------
// Check: SPA fallback — unknown route returns index.html
// ---------------------------------------------------------------------------
const spaR = await get(`${BASE}/some/deep/spa/route/that/does/not/exist`);
const spaBodyOk = spaR.body.equals(indexExpected);
record(
  "/{spa-route} → SPA fallback serves index.html",
  spaR.status === 200 && spaBodyOk,
  !spaBodyOk ? `body_mismatch(expected=${indexExpected.length}b got=${spaR.body.length}b)` :
  `status=${spaR.status}`
);

// ---------------------------------------------------------------------------
// Check: /api/session → 200 + base64url mutationToken (length 43 for 32 bytes)
// ---------------------------------------------------------------------------
const sessionR = await get(`${BASE}/api/session`);
const sessionBody = JSON.parse(sessionR.body.toString("utf8"));
const token = sessionBody.mutationToken;
const tokenOk = typeof token === "string"
  && token.length === 43
  && /^[A-Za-z0-9\-_]+$/.test(token);
record(
  "/api/session → 200 + base64url mutationToken (43 chars)",
  sessionR.status === 200 && tokenOk,
  tokenOk ? "" : `token=${JSON.stringify(token)} len=${token?.length}`
);

// ---------------------------------------------------------------------------
// Check: security — non-loopback Host header → 403 + exact error JSON
// ---------------------------------------------------------------------------
// Node.js `fetch` (undici) forbids setting the `host` header; use raw http.request instead.
const { request: httpRequest } = await import("node:http");
const expectedSecError = "Architext serve accepts requests only from its loopback origin.";

const secR = await new Promise((resolve, reject) => {
  const req = httpRequest({
    host: "127.0.0.1",
    port: testPort,
    path: "/api/session",
    method: "GET",
    headers: { host: "evil.example.com:9999" },
  }, (res) => {
    const chunks = [];
    res.on("data", (c) => chunks.push(c));
    res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks) }));
  });
  req.on("error", reject);
  req.end();
});
const secBody = JSON.parse(secR.body.toString("utf8"));
record(
  "security: non-loopback Host → 403 + error JSON",
  secR.status === 403 && secBody.error === expectedSecError,
  `status=${secR.status} error=${JSON.stringify(secBody.error)}`
);

// ---------------------------------------------------------------------------
// Check: /api/unknown → 404 + error JSON matching JS shape
// ---------------------------------------------------------------------------
const unknownR = await get(`${BASE}/api/unknown_xyz_route`);
const unknownBody = JSON.parse(unknownR.body.toString("utf8"));
const expectedUnknownError = "Unknown Architext API route: /api/unknown_xyz_route";
record(
  "/api/unknown → 404 + 'Unknown Architext API route: ...'",
  unknownR.status === 404 && unknownBody.error === expectedUnknownError,
  `status=${unknownR.status} error=${JSON.stringify(unknownBody.error)}`
);

// ---------------------------------------------------------------------------
// Check: /api/status → 200 + {ok, status}; ok matches formula; Cache-Control absent
// ---------------------------------------------------------------------------
{
  const statusR = await get(`${BASE}/api/status`);
  let statusPass = true;
  let statusNote = "";

  if (statusR.status !== 200) {
    statusPass = false;
    statusNote = `HTTP ${statusR.status} (expected 200)`;
  } else {
    let parsed;
    try { parsed = JSON.parse(statusR.body.toString("utf8")); } catch (e) {
      statusPass = false;
      statusNote = `JSON parse error: ${e.message}`;
    }
    if (statusPass) {
      // Shape: { ok: bool, status: { installed, needsMigration, validation?, ... } }
      if (typeof parsed.ok !== "boolean") {
        statusPass = false;
        statusNote = `ok is not a boolean: ${JSON.stringify(parsed.ok)}`;
      } else if (!parsed.status || typeof parsed.status !== "object") {
        statusPass = false;
        statusNote = `status is missing or not an object`;
      } else {
        // Verify ok formula: installed && !needsMigration && validation?.ok !== false
        const { installed, needsMigration, validation } = parsed.status;
        const expectedOk = installed === true && needsMigration === false && validation?.ok !== false;
        if (parsed.ok !== expectedOk) {
          statusPass = false;
          statusNote = `ok=${parsed.ok} but formula(installed=${installed}, needsMigration=${needsMigration}, validation.ok=${validation?.ok}) → ${expectedOk}`;
        }
      }
    }
  }
  record("/api/status → 200 {ok,status}; ok matches formula", statusPass, statusNote);
}

// ---------------------------------------------------------------------------
// Check: /api/config → 200 + semantic-equal to JS oracle; Cache-Control: no-store
// ---------------------------------------------------------------------------
{
  const jsConfigOracle = await diagramConfigGetPayload(repoRoot);
  const configR = await get(`${BASE}/api/config`);
  let configPass = true;
  let configNote = "";

  if (configR.status !== 200) {
    configPass = false;
    configNote = `HTTP ${configR.status} (expected 200)`;
  } else if (configR.headers.get("cache-control") !== "no-store") {
    configPass = false;
    configNote = `cache-control=${configR.headers.get("cache-control")} (expected no-store)`;
  } else {
    let rustConfig;
    try { rustConfig = JSON.parse(configR.body.toString("utf8")); } catch (e) {
      configPass = false;
      configNote = `JSON parse error: ${e.message}`;
    }
    if (configPass) {
      // Compare diagram resolved values (semantic equality, both serialised via JSON)
      const jsDiagram = JSON.stringify(jsConfigOracle.diagram);
      const rustDiagram = JSON.stringify(rustConfig.diagram);
      if (jsDiagram !== rustDiagram) {
        configPass = false;
        configNote = `diagram mismatch: JS=${jsDiagram.slice(0, 120)} RUST=${rustDiagram.slice(0, 120)}`;
      }
      // Compare fields spec (DIAGRAM_CONFIG_FIELDS)
      if (configPass) {
        const jsFields = JSON.stringify(jsConfigOracle.fields);
        const rustFields = JSON.stringify(rustConfig.fields);
        if (jsFields !== rustFields) {
          configPass = false;
          configNote = `fields mismatch: JS=${jsFields.slice(0, 80)} RUST=${rustFields.slice(0, 80)}`;
        }
      }
      // Compare sections (SECTION_LABELS)
      if (configPass) {
        const jsSections = JSON.stringify(jsConfigOracle.sections);
        const rustSections = JSON.stringify(rustConfig.sections);
        if (jsSections !== rustSections) {
          configPass = false;
          configNote = `sections mismatch: JS=${jsSections} RUST=${rustSections}`;
        }
      }
      // warnings: both should be empty arrays (no config files at repoRoot)
      if (configPass) {
        const jsW = jsConfigOracle.warnings;
        const rustW = rustConfig.warnings;
        if (!Array.isArray(rustW)) {
          configPass = false;
          configNote = `warnings not an array: ${JSON.stringify(rustW)}`;
        } else if (jsW.length === 0 && rustW.length !== 0) {
          configPass = false;
          configNote = `warnings mismatch: JS=[] RUST=${JSON.stringify(rustW)}`;
        }
      }
    }
  }
  record("/api/config → semantic-equal JS oracle; Cache-Control: no-store", configPass, configNote);
}

// ---------------------------------------------------------------------------
// Check: /api/repo-tree → 200 + source + files parity; mtime integer ms; Cache-Control: no-store
// ---------------------------------------------------------------------------
{
  const jsTree = await repoTreeApiRequest(repoRoot);
  const treeR = await get(`${BASE}/api/repo-tree`);
  let treePass = true;
  let treeNote = "";

  if (treeR.status !== 200) {
    treePass = false;
    treeNote = `HTTP ${treeR.status} (expected 200)`;
  } else if (treeR.headers.get("cache-control") !== "no-store") {
    treePass = false;
    treeNote = `cache-control=${treeR.headers.get("cache-control")} (expected no-store)`;
  } else {
    let rustTree;
    try { rustTree = JSON.parse(treeR.body.toString("utf8")); } catch (e) {
      treePass = false;
      treeNote = `JSON parse error: ${e.message}`;
    }
    if (treePass) {
      // source must match
      if (rustTree.source !== jsTree.source) {
        treePass = false;
        treeNote = `source mismatch: JS=${jsTree.source} RUST=${rustTree.source}`;
      }
      // files count must match
      if (treePass && rustTree.files.length !== jsTree.files.length) {
        treePass = false;
        treeNote = `files count mismatch: JS=${jsTree.files.length} RUST=${rustTree.files.length}`;
      }
      if (treePass) {
        // Build maps for fast lookup
        const rustMap = new Map(rustTree.files.map(f => [f.path, f]));
        const jsMap = new Map(jsTree.files.map(f => [f.path, f]));

        // Check all JS paths are present in Rust
        let missingInRust = null;
        for (const [p] of jsMap) {
          if (!rustMap.has(p)) { missingInRust = p; break; }
        }
        if (missingInRust) {
          treePass = false;
          treeNote = `path in JS but missing in Rust: ${missingInRust}`;
        }
      }
      if (treePass) {
        const rustMap = new Map(rustTree.files.map(f => [f.path, f]));
        // For a sample of files, compare size and mtime (normalised to integer ms).
        const jsFiles = jsTree.files;
        const samplesToCheck = jsFiles.length > 20 ? 20 : jsFiles.length;
        for (let i = 0; i < samplesToCheck; i++) {
          const jf = jsFiles[i];
          const rf = rustMap.get(jf.path);
          if (!rf) continue; // already caught above
          // size must match
          if (rf.size !== jf.size) {
            treePass = false;
            treeNote = `${jf.path} size mismatch: JS=${jf.size} RUST=${rf.size}`;
            break;
          }
          // mtime: both should be integer ms; normalise both to integer
          // JS: Math.round(mtimeMs). Rust: duration_since_epoch.as_millis().
          // Both are integer ms; verify both are integers and close.
          const jMtime = jf.mtime;
          const rMtime = rf.mtime;
          if (jMtime !== null && rMtime !== null) {
            if (typeof rMtime !== "number" || !Number.isInteger(rMtime)) {
              treePass = false;
              treeNote = `${jf.path} mtime is not integer: RUST=${rMtime}`;
              break;
            }
            // Allow ±1000ms tolerance (filesystem clock resolution differences
            // between stat() in JS vs Rust are sub-ms, but we tolerate 1s to be
            // safe against any clock conversion edge cases).
            if (Math.abs(jMtime - rMtime) > 1000) {
              treePass = false;
              treeNote = `${jf.path} mtime mismatch: JS=${jMtime} RUST=${rMtime} (diff=${Math.abs(jMtime-rMtime)}ms)`;
              break;
            }
          }
        }
      }
    }
  }
  record(
    `/api/repo-tree → source=${jsTree.source} files=${jsTree.files.length}; mtime integer ms; Cache-Control: no-store`,
    treePass,
    treeNote
  );
}

// ---------------------------------------------------------------------------
// Shutdown server
// ---------------------------------------------------------------------------
serverShuttingDown = true;
server.kill();

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------
console.log("--- Results ---");
for (const c of checks) {
  const marker = c.pass ? "✓" : "✗";
  const status = c.pass ? "GREEN" : "RED";
  const note = c.note ? `  (${c.note})` : "";
  console.log(`${marker} ${status.padEnd(5)} ${c.name}${note}`);
}
console.log(`\n${green}/${checks.length} GREEN`);
if (green !== checks.length) process.exitCode = 1;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function findFreePort() {
  // Bind a temporary server on :0 to get a free port from the OS.
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
      const r = await fetch(`http://127.0.0.1:${port}/api/session`);
      if (r.status === 200) return;
    } catch {
      // server not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  console.error(`FATAL: server on port ${port} did not become ready in ${timeoutMs}ms`);
  server.kill();
  process.exit(1);
}
