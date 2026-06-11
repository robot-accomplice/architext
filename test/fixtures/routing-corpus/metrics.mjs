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

// Performance ratchet counters: deterministic work-volume measures collected from the
// planner's stats hooks. Exact for fixed inputs on every machine, so they gate strictly
// with zero CI flake. They catch the historically dominant regression class (candidate
// volume blowups); pure per-operation slowdowns are caught coarsely by the calibrated
// wall-time ratio below.
export const PERF_GATED_COUNTERS = ["edgesPlanned", "cheapCandidateCount", "gridRouteCalls"];

function planCorpusFlow(flow, views, stats) {
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
  return planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    style: "orthogonal",
    diagnostics: true,
    stats,
    ...diagramLayoutFor(view, relationships.length)
  });
}

// One corpus pass producing both the quality metrics and the performance measures, so
// the fitness gate and the perf gate can never observe different plans.
export function computeCorpusMetricsAndPerf() {
  const views = loadJson("views.json").views;
  const flows = loadJson("flows.json").flows;
  const metricsByFlow = {};
  const perfByFlow = {};

  // Calibration yardstick: the smallest flow, planned cold once up front (untimed)
  // so every timed calibration run below hits the raw-route cache — an identical
  // workload on every machine and every run.
  const smallest = [...flows].sort((a, b) => a.steps.length - b.steps.length)[0];
  planCorpusFlow(smallest, views, {});

  // The calibration runs are INTERLEAVED with the corpus flows so both timers
  // sample the same load timeline: a parallel test suite (or CI runner) that
  // time-slices the corpus pass slows the calibration identically, and the
  // corpus/calibration ratio stays valid under load — measuring it after the
  // corpus let suite parallelism inflate the ratio 2x and flake the gate.
  let corpusWallMs = 0;
  let calibrationMs = 0;
  for (const flow of flows) {
    const stats = {};
    const corpusStart = performance.now();
    const plan = planCorpusFlow(flow, views, stats);
    corpusWallMs += performance.now() - corpusStart;
    const metrics = { ...plan.diagnostics.metrics, crossings: totalCrossings(plan.routes) };
    metricsByFlow[flow.id] = Object.fromEntries(GATED_METRICS.map((key) => [key, metrics[key] ?? 0]));
    perfByFlow[flow.id] = Object.fromEntries(PERF_GATED_COUNTERS.map((key) => [key, stats[key] ?? 0]));
    const calibrationStart = performance.now();
    planCorpusFlow(smallest, views, {});
    calibrationMs += performance.now() - calibrationStart;
  }
  calibrationMs = Math.max(0.1, calibrationMs);

  return {
    metrics: metricsByFlow,
    perf: {
      flows: perfByFlow,
      corpusWallMs: Math.round(corpusWallMs),
      calibrationMs: Math.round(calibrationMs * 10) / 10,
      wallRatio: Math.round((corpusWallMs / calibrationMs) * 10) / 10
    }
  };
}

export function computeCorpusMetrics() {
  return computeCorpusMetricsAndPerf().metrics;
}
