// Computes routing-quality metrics for every flow in the sanitized corpus, using the
// production planning + diagnostics path. Shared by the fitness test and the baseline
// regenerator so the gate and the snapshot can never drift apart.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { planDiagram } from "../../../viewer/src/routing/planDiagram.js";
import { crossingsBetween } from "../../../viewer/src/routing/routeEdges.js";
import { diagramLayoutFor } from "../../../viewer/src/presentation/diagramLayout.js";

const DIR = dirname(fileURLToPath(import.meta.url));
const FLOW_VIEW_TYPES = new Set(["system-map", "flow-explorer", "workflow", "dataflow"]);

// Metrics that are gated. routes is a structural invariant; the rest are quality costs
// where lower is better. findings/constraints/hops are derived/contextual and excluded.
//
// `crossings` is the TOTAL of rendered cross-edge intersections (every route pair), NOT
// just the self-crossings pairInternalCrossings counts. The diagnostics object never
// exposed this aggregate, so a change could redistribute crossings — clean one diagram,
// dirty another — with zero net movement in any gated metric. That blind spot let a
// capacity change regress two pristine flows from 0 to 4 crossings undetected. Gating the
// aggregate closes it: any net increase, anywhere, now trips the ratchet.
export const GATED_METRICS = [
  "routes",
  "bends",
  "crossings",
  "pairInternalCrossings",
  "laneOrderViolations",
  "closeParallelRuns",
  "sharedSegments",
  "repeatedCrossings"
];

// Total rendered cross-edge intersections across every route pair — the aggregate the
// diagnostics object omits. Uses the same crossingsBetween the distribution tests use.
function totalCrossings(routes) {
  const all = [...routes.values()];
  let total = 0;
  for (let a = 0; a < all.length; a += 1) {
    for (let b = a + 1; b < all.length; b += 1) total += crossingsBetween(all[a], all[b]);
  }
  return total;
}

function loadJson(name) {
  return JSON.parse(readFileSync(join(DIR, name), "utf8"));
}

function flowFitsView(flow, view) {
  const ids = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  return flow.steps.length > 0 && flow.steps.every((step) => ids.has(step.from) && ids.has(step.to));
}

// The view a flow renders in: its first authored (non-system-map) projection, else the
// first compatible view. Deterministic, mirroring the viewer's flows-mode default.
export function renderedViewFor(flow, views) {
  const compatible = views.filter((view) => FLOW_VIEW_TYPES.has(view.type) && flowFitsView(flow, view));
  return compatible.find((view) => view.type !== "system-map") ?? compatible[0] ?? null;
}

export function computeCorpusMetrics() {
  const views = loadJson("views.json").views;
  const flows = loadJson("flows.json").flows;
  const result = {};
  for (const flow of flows) {
    const view = renderedViewFor(flow, views);
    if (!view) throw new Error(`No rendered view for corpus flow "${flow.id}"`);
    const relationships = flow.steps.map((step, index) => ({
      id: step.id,
      from: step.from,
      to: step.to,
      relationshipType: "flow",
      displayIndex: index + 1,
      kind: step.kind,
      returnOf: step.returnOf,
      outcome: step.outcome
    }));
    const plan = planDiagram({
      view,
      relationships,
      visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
      style: "orthogonal",
      diagnostics: true,
      ...diagramLayoutFor(view, relationships.length)
    });
    const metrics = { ...plan.diagnostics.metrics, crossings: totalCrossings(plan.routes) };
    result[flow.id] = Object.fromEntries(GATED_METRICS.map((key) => [key, metrics[key] ?? 0]));
  }
  return result;
}
