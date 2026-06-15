// Differential parity GATE (Phase 1B): for every flow x view, compute the JS
// engine's fingerprint (planDiagram) and the Rust engine's fingerprint (the
// WASM plan()), using the SAME hashing, and diff. A flow is GREEN only when the
// Rust output matches the JS output byte-for-byte. Drives the engine port
// flow-by-flow: each flow stays RED until its Rust fingerprint equals JS.
// Exits non-zero if any flow is RED, so it gates. Both this harness and the
// JS-only route-parity-fingerprint.mjs are transitional — deleted with the JS
// engine once the port is 100% GREEN.
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const { enumerateFlowPlanRequests } = await import(path.join(repoRoot, "src/adapters/http/plan-precompute.mjs"));
const { planDiagram } = await import(path.join(repoRoot, "viewer/src/routing/planDiagram.js"));
const wasm = await import(path.join(repoRoot, "crates/architext-routing/pkg/architext_routing.js"));

// Works for both a JS Map (`[...map]` -> entries) and the Rust wire shape
// (`plan.routes` is an array of [id, route] pairs) — identical destructuring.
function fingerprint(plan) {
  const hash = createHash("sha256");
  const routes = [...plan.routes].sort(([a], [b]) => a.localeCompare(b));
  for (const [id, route] of routes) {
    hash.update(id).update(route.d).update(JSON.stringify(route.points)).update(String(route.labelX)).update(String(route.labelY));
  }
  return hash.digest("hex");
}

const requests = await enumerateFlowPlanRequests({ dataDir: path.join(repoRoot, "docs/architext/data"), layoutConfig: undefined });

let green = 0;
const rows = [];
for (const req of requests) {
  const flow = `${req.flowId}@${req.viewId}`;
  let rustFp = "ERROR";
  let jsFp = "ERROR";
  try {
    jsFp = fingerprint(planDiagram(req.planInput));
  } catch (error) {
    jsFp = `JS-ERROR: ${error.message}`;
  }
  try {
    // Phase 1B note: once plan() consumes its input, this must serialize
    // planInput through the shared planRequest wire form. While the Rust side
    // echoes a fixture, the input content is ignored.
    rustFp = fingerprint(JSON.parse(wasm.plan(JSON.stringify(req.planInput))));
  } catch (error) {
    rustFp = `RUST-ERROR: ${error.message}`;
  }
  const ok = jsFp === rustFp && jsFp.length === 64;
  if (ok) green += 1;
  rows.push({ flow, status: ok ? "GREEN" : "RED", js: jsFp.slice(0, 12), rust: rustFp.slice(0, 12) });
}

for (const r of rows) console.log(`${r.status === "GREEN" ? "✓" : "✗"} ${r.status.padEnd(5)} ${r.flow.padEnd(40)} js=${r.js} rust=${r.rust}`);
console.log(`\n${green}/${requests.length} GREEN`);
if (green !== requests.length) process.exitCode = 1;
