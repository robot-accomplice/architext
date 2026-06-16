// serve-parity-rust.mjs
//
// HTTP-level parity gate for the Rust serve adapter (Phase 2A slice 1).
//
// DOES NOT boot the flaky Node serve-lifecycle. Instead:
//   1. Builds + spawns the Rust `architext-serve` binary on an ephemeral port.
//   2. Runs a suite of HTTP checks against it.
//   3. Derives oracles directly from the JS source-of-truth (plan-precompute.mjs,
//      planDiagram.js, planCodec.js) — no Node server involved.
//   4. Reports per-check GREEN/RED + N/total, exits nonzero on any RED.
//
// Checks:
//   /api/plan/{hash}   — hit: body byte-equals JS oracle; miss: 200 {miss:true}
//   /data/{path}       — body bytes + content-type + cache-control
//   /                  — index.html served
//   /{spa-route}       — SPA fallback returns index.html
//   /api/session       — 200 + base64url mutationToken of correct length
//   security           — non-loopback Host/Origin → 403 exact error JSON
//   /api/unknown       — 404 + error JSON shape
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
