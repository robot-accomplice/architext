// PHASE 1B: replace the single-flow echo check with a loop over ALL requests,
// computing both the JS fingerprint (planDiagram) and the Rust fingerprint
// (wasm.plan) per flow, and asserting equality. Drive the engine port flow-by-
// flow: each flow stays RED until its Rust fingerprint equals its JS fingerprint.

// Differential parity: hash the Rust engine's plan (via WASM plan()) with the
// SAME hashing used for the JS engine, and diff. Identical fingerprints prove
// the Rust output matches the JS output byte-for-byte.
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const { enumerateFlowPlanRequests } = await import(path.join(repoRoot, "src/adapters/http/plan-precompute.mjs"));
const { planInputKey } = await import(path.join(repoRoot, "viewer/src/routing/planKey.js"));
const wasm = await import(path.join(repoRoot, "crates/architext-routing/pkg/architext_routing.js"));

function fingerprint(plan) {
  const hash = createHash("sha256");
  const routes = [...plan.routes].sort(([a], [b]) => a.localeCompare(b));
  for (const [id, route] of routes) {
    hash.update(id).update(route.d).update(JSON.stringify(route.points)).update(String(route.labelX)).update(String(route.labelY));
  }
  return hash.digest("hex");
}

const requests = await enumerateFlowPlanRequests({ dataDir: path.join(repoRoot, "docs/architext/data"), layoutConfig: undefined });
const req = requests[0];
const planJson = wasm.plan(JSON.stringify({ ...req.planInput, __key: planInputKey(req.planInput) }));
const rustPlan = JSON.parse(planJson);
console.log(JSON.stringify({ flow: `${req.flowId}@${req.viewId}`, rust: fingerprint(rustPlan) }, null, 1));
