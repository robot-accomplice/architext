// SCRATCH HARNESS (1.7.0 cutover investigation) — Rust-vs-JS per-edge route diff.
//
// For a given view+flow (or structural C4/Deployment view), runs BOTH engines on
// the SAME serialized input and diffs per edge:
//   - mount face at each end (derived from route.points vs node rect, JS sideForPoint)
//   - d-string geometry (byte equality)
//   - points array
//   - engine-neutral metrics (JS diagnosePlannedRoutes run on BOTH plans):
//     crossings (total rendered), bends, laneOrderViolations, pairInternalCrossings.
//
// Reuses serializePlanInput from route-parity-rust.mjs's contract and the WASM plan().
//
// Usage:
//   node viewer/tools/route-diff-harness.mjs corpus <flowId>
//   node viewer/tools/route-diff-harness.mjs flow <flowId> <viewId>
//   node viewer/tools/route-diff-harness.mjs structural <viewId>
import path from "node:path";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");

const { planDiagram } = await import(path.join(repoRoot, "viewer/src/routing/planDiagram.js"));
const { diagnosePlannedRoutes, sideForPoint } = await import(path.join(repoRoot, "viewer/src/routing/routeDiagnostics.js"));
const { crossingsBetween } = await import(path.join(repoRoot, "viewer/src/routing/routeEdges.js"));
const { diagramLayoutFor } = await import(path.join(repoRoot, "viewer/src/presentation/diagramLayout.js"));
const { buildFlowRelationships } = await import(path.join(repoRoot, "viewer/src/presentation/planRequest.js"));
const { relationshipLabel } = await import(path.join(repoRoot, "viewer/src/routing/relationshipLabels.js"));
const wasm = await import(path.join(repoRoot, "crates/architext-routing/pkg/architext_routing.js"));

function loadJson(p) { return JSON.parse(readFileSync(p, "utf8")); }

// --- serializePlanInput (copied verbatim from route-parity-rust.mjs contract) ---
function serializePlanInput(input, diagnostics = false) {
  return JSON.stringify({
    view: { lanes: input.view.lanes.map((lane) => ({ id: lane.id, nodeIds: lane.nodeIds })) },
    relationships: input.relationships.map((r) => ({
      id: r.id, from: r.from, to: r.to, label: r.label ?? null,
      relationshipType: r.relationshipType ?? null, stepId: r.stepId ?? null,
      flowId: r.flowId ?? null, kind: r.kind ?? null, returnOf: r.returnOf ?? null,
      outcome: r.outcome ?? null, displayIndex: r.displayIndex ?? 0,
      preferredStartSide: r.preferredStartSide ?? null, preferredEndSide: r.preferredEndSide ?? null,
    })),
    visibleNodeIds: input.visibleNodeIds instanceof Set ? Array.from(input.visibleNodeIds) : (input.visibleNodeIds ?? []),
    nodeWidth: input.nodeWidth, nodeHeight: input.nodeHeight, laneWidth: input.laneWidth,
    rowGap: input.rowGap, marginX: input.marginX, marginY: input.marginY,
    minCanvasWidth: input.minCanvasWidth, minCanvasHeight: input.minCanvasHeight,
    canvasExtraWidth: input.canvasExtraWidth, canvasExtraHeight: input.canvasExtraHeight,
    extraNodeRects: input.extraNodeRects instanceof Map ? Array.from(input.extraNodeRects.entries()) : (input.extraNodeRects ?? []),
    extraLaneIndexByNode: input.extraLaneIndexByNode instanceof Map ? Array.from(input.extraLaneIndexByNode.entries()) : (input.extraLaneIndexByNode ?? []),
    extraRowIndexByNode: input.extraRowIndexByNode instanceof Map ? Array.from(input.extraRowIndexByNode.entries()) : (input.extraRowIndexByNode ?? []),
    scoreEdgeProximity: Boolean(input.scoreEdgeProximity),
    style: input.style ?? "orthogonal",
    diagnostics,
  });
}

// Rust plan() returns the wire shape (entries arrays). Rehydrate to Map-based plan
// so JS diagnostics / sideForPoint can consume it identically to a JS plan.
function rehydrateRustPlan(wirePlan) {
  const toMap = (entries) => new Map(entries);
  return {
    ...wirePlan,
    nodeRects: toMap(wirePlan.nodeRects),
    routes: toMap(wirePlan.routes),
    laneIndexByNode: toMap(wirePlan.laneIndexByNode),
    rowIndexByNode: toMap(wirePlan.rowIndexByNode),
    labelBoxes: toMap(wirePlan.labelBoxes ?? []),
  };
}

function totalCrossings(routesMap) {
  const all = [...routesMap.values()];
  let total = 0;
  for (let a = 0; a < all.length; a += 1)
    for (let b = a + 1; b < all.length; b += 1) total += crossingsBetween(all[a], all[b]);
  return total;
}

// Per-edge mount faces from route endpoints vs the from/to node rects.
function edgeMounts(plan, relationships) {
  const byId = new Map(relationships.map((r) => [r.id, r]));
  const out = new Map();
  for (const [id, route] of plan.routes) {
    const rel = byId.get(id);
    if (!rel || !route.points?.length) continue;
    const fromRect = plan.nodeRects.get(rel.from);
    const toRect = plan.nodeRects.get(rel.to);
    out.set(id, {
      from: rel.from, to: rel.to,
      sourceSide: fromRect ? sideForPoint(fromRect, route.points[0]) : "?",
      targetSide: toRect ? sideForPoint(toRect, route.points[route.points.length - 1]) : "?",
      bends: Math.max(0, route.points.length - 2),
      d: route.d,
    });
  }
  return out;
}

function metrics(plan, relationships) {
  const diag = diagnosePlannedRoutes(plan, relationships, {});
  return {
    routes: plan.routes.size,
    crossings: totalCrossings(plan.routes),
    bends: diag.metrics.bends ?? 0,
    pairInternalCrossings: diag.metrics.pairInternalCrossings ?? 0,
    laneOrderViolations: diag.metrics.laneOrderViolations ?? 0,
  };
}

// --- build plan inputs per mode ---
function buildCorpusInput(flowId) {
  const dir = path.join(repoRoot, "test/fixtures/routing-corpus");
  const views = loadJson(path.join(dir, "views.json")).views;
  const flows = loadJson(path.join(dir, "flows.json")).flows;
  const flow = flows.find((f) => f.id === flowId);
  if (!flow) throw new Error(`corpus flow not found: ${flowId}`);
  const FLOW_VIEW_TYPES = new Set(["system-map", "flow-explorer", "workflow", "dataflow"]);
  const fits = (v) => flow.steps.length > 0 && flow.steps.every((s) => {
    const ids = new Set(v.lanes.flatMap((l) => l.nodeIds)); return ids.has(s.from) && ids.has(s.to);
  });
  const compatible = views.filter((v) => FLOW_VIEW_TYPES.has(v.type) && fits(v));
  const view = compatible.find((v) => v.type !== "system-map") ?? compatible[0];
  const relationships = flow.steps.map((step, index) => ({
    id: step.id, from: step.from, to: step.to, relationshipType: "flow",
    displayIndex: index + 1, kind: step.kind, returnOf: step.returnOf, outcome: step.outcome,
  }));
  const visibleNodeIds = new Set(view.lanes.flatMap((l) => l.nodeIds));
  return {
    view, relationships, visibleNodeIds, style: "orthogonal",
    ...diagramLayoutFor(view, relationships.length),
  };
}

function buildRealFlowInput(flowId, viewId) {
  const dir = path.join(repoRoot, "docs/architext/data");
  const views = loadJson(path.join(dir, "views.json")).views;
  const flows = loadJson(path.join(dir, "flows.json")).flows;
  const flow = flows.find((f) => f.id === flowId);
  const view = views.find((v) => v.id === viewId);
  if (!flow || !view) throw new Error(`flow/view not found: ${flowId}/${viewId}`);
  const relationships = buildFlowRelationships(flow, view);
  const visibleNodeIds = new Set(view.lanes.flatMap((l) => l.nodeIds));
  return { view, relationships, visibleNodeIds, style: "orthogonal", ...diagramLayoutFor(view, relationships.length) };
}

function buildStructuralInput(viewId) {
  const dir = path.join(repoRoot, "docs/architext/data");
  const views = loadJson(path.join(dir, "views.json")).views;
  const nodes = loadJson(path.join(dir, "nodes.json")).nodes;
  const nodesById = new Map(nodes.map((n) => [n.id, n]));
  const view = views.find((v) => v.id === viewId);
  if (!view) throw new Error(`view not found: ${viewId}`);
  const visibleNodeIds = new Set(view.lanes.flatMap((l) => l.nodeIds));
  // Mirror main.tsx structuralRelationships exactly.
  const relationships = Array.from(visibleNodeIds).flatMap((nodeId) => {
    const node = nodesById.get(nodeId);
    return (node?.dependencies ?? [])
      .filter((dep) => visibleNodeIds.has(dep))
      .map((dep) => {
        const to = nodesById.get(dep);
        return {
          id: `${nodeId}-${dep}`, from: nodeId, to: dep,
          label: relationshipLabel(node, to), relationshipType: "structural", toType: to?.type,
        };
      });
  });
  return { view, relationships, visibleNodeIds, style: "orthogonal", ...diagramLayoutFor(view, relationships.length) };
}

// --- main ---
const [mode, a, b] = process.argv.slice(2);
let input;
if (mode === "corpus") input = buildCorpusInput(a);
else if (mode === "flow") input = buildRealFlowInput(a, b);
else if (mode === "structural") input = buildStructuralInput(a);
else { console.error("usage: corpus <flowId> | flow <flowId> <viewId> | structural <viewId>"); process.exit(2); }

const jsPlan = planDiagram(input);
const rustPlan = rehydrateRustPlan(JSON.parse(wasm.plan(serializePlanInput(input))));

const jsMounts = edgeMounts(jsPlan, input.relationships);
const rustMounts = edgeMounts(rustPlan, input.relationships);

console.log(`# mode=${mode} ${a ?? ""} ${b ?? ""}  view=${input.view.id}  edges=${input.relationships.length}`);
console.log(`# JS   metrics: ${JSON.stringify(metrics(jsPlan, input.relationships))}`);
console.log(`# Rust metrics: ${JSON.stringify(metrics(rustPlan, input.relationships))}`);
console.log(`#`);
console.log(`# per-edge (★ = divergent):`);
const allIds = [...new Set([...jsMounts.keys(), ...rustMounts.keys()])];
for (const id of allIds) {
  const j = jsMounts.get(id), r = rustMounts.get(id);
  if (!j || !r) { console.log(`★ ${id}  MISSING js=${!!j} rust=${!!r}`); continue; }
  const mountDiff = j.sourceSide !== r.sourceSide || j.targetSide !== r.targetSide;
  const dDiff = j.d !== r.d;
  const mark = (mountDiff || dDiff) ? "★" : " ";
  console.log(`${mark} ${id.padEnd(18)} ${j.from}->${j.to}`);
  console.log(`    JS   src=${j.sourceSide.padEnd(6)} tgt=${j.targetSide.padEnd(6)} bends=${j.bends}  d=${j.d}`);
  console.log(`    Rust src=${r.sourceSide.padEnd(6)} tgt=${r.targetSide.padEnd(6)} bends=${r.bends}  d=${r.d}`);
}
