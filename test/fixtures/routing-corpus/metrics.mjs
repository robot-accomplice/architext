// Computes routing-quality metrics for every flow in the sanitized corpus, using the
// production planning + diagnostics path. Shared by the fitness test and the baseline
// regenerator so the gate and the snapshot can never drift apart.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { planDiagram } from "../../../viewer/src/routing/planDiagram.js";
import { diagramLayoutFor } from "../../../viewer/src/presentation/diagramLayout.js";

const DIR = dirname(fileURLToPath(import.meta.url));
const FLOW_VIEW_TYPES = new Set(["system-map", "flow-explorer", "workflow", "dataflow"]);

// Metrics that are gated. routes is a structural invariant; the rest are quality costs
// where lower is better. findings/constraints/hops are derived/contextual and excluded.
export const GATED_METRICS = [
  "routes",
  "bends",
  "pairInternalCrossings",
  "laneOrderViolations",
  "closeParallelRuns",
  "sharedSegments",
  "repeatedCrossings"
];

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
    const metrics = plan.diagnostics.metrics;
    result[flow.id] = Object.fromEntries(GATED_METRICS.map((key) => [key, metrics[key] ?? 0]));
  }
  return result;
}
