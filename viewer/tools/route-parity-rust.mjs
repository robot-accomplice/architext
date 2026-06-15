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

// Serialize a planInput object to the wire shape the Rust PlanDiagramInput
// expects. JS planInput uses Set (visibleNodeIds) and Map (extraNodeRects,
// extraLaneIndexByNode, extraRowIndexByNode) which JSON.stringify loses.
function serializePlanInput(input) {
  return JSON.stringify({
    view: { lanes: input.view.lanes.map((lane) => ({ id: lane.id, nodeIds: lane.nodeIds })) },
    relationships: input.relationships.map((r) => ({
      id: r.id,
      from: r.from,
      to: r.to,
      label: r.label ?? null,
      relationshipType: r.relationshipType ?? null,
      stepId: r.stepId ?? null,
      flowId: r.flowId ?? null,
      kind: r.kind ?? null,
      returnOf: r.returnOf ?? null,
      outcome: r.outcome ?? null,
      displayIndex: r.displayIndex ?? 0,
      preferredStartSide: r.preferredStartSide ?? null,
      preferredEndSide: r.preferredEndSide ?? null,
    })),
    // Set → plain array (insertion order preserved)
    visibleNodeIds: input.visibleNodeIds instanceof Set
      ? Array.from(input.visibleNodeIds)
      : (input.visibleNodeIds ?? []),
    nodeWidth: input.nodeWidth,
    nodeHeight: input.nodeHeight,
    laneWidth: input.laneWidth,
    rowGap: input.rowGap,
    marginX: input.marginX,
    marginY: input.marginY,
    minCanvasWidth: input.minCanvasWidth,
    minCanvasHeight: input.minCanvasHeight,
    canvasExtraWidth: input.canvasExtraWidth,
    canvasExtraHeight: input.canvasExtraHeight,
    // Map → [[key, value], ...] entries arrays
    extraNodeRects: input.extraNodeRects instanceof Map
      ? Array.from(input.extraNodeRects.entries())
      : (input.extraNodeRects ?? []),
    extraLaneIndexByNode: input.extraLaneIndexByNode instanceof Map
      ? Array.from(input.extraLaneIndexByNode.entries())
      : (input.extraLaneIndexByNode ?? []),
    extraRowIndexByNode: input.extraRowIndexByNode instanceof Map
      ? Array.from(input.extraRowIndexByNode.entries())
      : (input.extraRowIndexByNode ?? []),
    scoreEdgeProximity: Boolean(input.scoreEdgeProximity),
    style: input.style ?? "orthogonal",
    diagnostics: false,
  });
}

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
    rustFp = fingerprint(JSON.parse(wasm.plan(serializePlanInput(req.planInput))));
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
