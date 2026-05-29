import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";

const ROBO = "/Users/jmachen/code/roboticus/docs/architext/data";
const flows = JSON.parse(readFileSync(`${ROBO}/flows.json`, "utf8")).flows;
const views = JSON.parse(readFileSync(`${ROBO}/views.json`, "utf8")).views;

function planInteractiveTurn() {
  const view = views.find((v) => v.id === "agent-turn-flow");
  const flow = flows.find((f) => f.id === "interactive-turn");
  const relationships = flow.steps.map((step, index) => ({
    id: step.id, from: step.from, to: step.to,
    label: `${index + 1}. ${step.action}`, summary: step.summary,
    relationshipType: "flow", stepId: step.id, flowId: flow.id,
    displayIndex: index + 1, kind: step.kind, returnOf: step.returnOf,
    stepKind: step.kind, outcome: step.outcome,
    componentFrom: step.from, componentTo: step.to
  }));
  const visibleNodeIds = new Set(flow.steps.flatMap((s) => [s.from, s.to]));
  return planDiagram({
    view, relationships, visibleNodeIds,
    nodeWidth: 136, nodeHeight: 54, laneWidth: 240, rowGap: 176,
    marginX: 228, marginY: 104, minCanvasWidth: 0, minCanvasHeight: 560,
    canvasExtraWidth: 180, canvasExtraHeight: 88, style: "orthogonal", diagnostics: true
  });
}

function segments(points) {
  const out = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const a = points[i], b = points[i + 1];
    if (a.y === b.y) out.push({ o: "h", line: a.y, min: Math.min(a.x, b.x), max: Math.max(a.x, b.x) });
    else if (a.x === b.x) out.push({ o: "v", line: a.x, min: Math.min(a.y, b.y), max: Math.max(a.y, b.y) });
  }
  return out;
}
function crossingsBetween(A, B) {
  const sa = segments(A), sb = segments(B), seen = new Set();
  for (const l of sa) for (const r of sb) {
    if (l.o === r.o) continue;
    const h = l.o === "h" ? l : r, v = l.o === "h" ? r : l;
    if (v.line > h.min && v.line < h.max && h.line > v.min && h.line < v.max) seen.add(`${v.line},${h.line}`);
  }
  return seen.size;
}
function crossingsForId(plan, id) {
  const me = plan.routes.get(id).points;
  let total = 0;
  for (const [other, route] of plan.routes) if (other !== id) total += crossingsBetween(me, route.points);
  return total;
}

test("reciprocal pair request-model/model-response routes crossing-free (legibility)", () => {
  const plan = planInteractiveTurn();
  // WHY: routing this pair through the hub's crowded southern fan-out is illegible;
  // a clean route (the open surface) costs zero crossings. Currently 8 (4 each).
  assert.equal(crossingsForId(plan, "request-model"), 0, "request-model must not cross other edges");
  assert.equal(crossingsForId(plan, "model-response"), 0, "model-response must not cross other edges");
});

test("no surface is left over capacity", () => {
  const plan = planInteractiveTurn();
  // WHY: an over-capacity surface forces mounts closer than legible; the model
  // must relieve it by moving the marginal endpoint to a clean surface.
  const overCapacity = (plan.diagnostics?.findings ?? plan.diagnostics ?? []).filter((d) => d.code === "surface-over-capacity");
  assert.equal(overCapacity.length, 0, `over-capacity surfaces: ${JSON.stringify(overCapacity)}`);
});

test("planning is idempotent (byte-identical on replan)", () => {
  const a = planInteractiveTurn();
  const b = planInteractiveTurn();
  const dump = (plan) => [...plan.routes.entries()]
    .map(([id, r]) => `${id}:${r.points.map((p) => `${p.x},${p.y}`).join("|")}`).sort().join("\n");
  assert.equal(dump(a), dump(b), "two plans of the same view must be identical");
});
