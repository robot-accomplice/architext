// Route-parity fingerprint harness for behavior-preserving routing changes.
//
// Plans every flow x compatible-flows-view pair in a data directory and prints
// a sha256 fingerprint per plan (route points, d strings, label positions).
// Capture before a change, capture after, diff the JSON: identical fingerprints
// prove the change moved no route by a single pixel. Used to verify the
// separation-pass and winner-selection optimizations byte-for-byte.
//
// Usage:
//   node viewer/tools/route-parity-fingerprint.mjs --data-dir <dir>
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const { enumerateFlowPlanRequests } = await import(path.join(repoRoot, "src/adapters/http/plan-precompute.mjs"));
const { planDiagram } = await import(path.join(repoRoot, "viewer/src/routing/planDiagram.js"));

const dataDirArg = process.argv.indexOf("--data-dir");
const dataDir = dataDirArg >= 0 ? path.resolve(process.argv[dataDirArg + 1]) : path.join(repoRoot, "docs", "architext", "data");
const requests = await enumerateFlowPlanRequests({ dataDir, layoutConfig: undefined });
const t0 = performance.now();
const fingerprints = {};
for (const request of requests) {
  const plan = planDiagram(request.planInput);
  const hash = createHash("sha256");
  for (const [id, route] of [...plan.routes.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    hash.update(id).update(route.d).update(JSON.stringify(route.points)).update(String(route.labelX)).update(String(route.labelY));
  }
  fingerprints[`${request.flowId}@${request.viewId}`] = hash.digest("hex");
}
console.log(JSON.stringify({ totalMs: Math.round(performance.now() - t0), plans: requests.length, fingerprints }, null, 1));
