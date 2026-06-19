import assert from "node:assert/strict";
import test from "node:test";
import { routeEdges } from "../viewer/src/routing/routeEdges.js";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";

// Unique geometry per test so the module-level raw-route cache from other test
// files (or the sibling test) can never satisfy the lookup and skip the
// planning loop that progress reporting instruments.
function progressInput(offsetY) {
  const nodeRects = new Map([
    ["pa", { x: 40, y: offsetY, width: 136, height: 54 }],
    ["pb", { x: 460, y: offsetY, width: 136, height: 54 }],
    ["pc", { x: 250, y: offsetY + 140, width: 136, height: 54 }]
  ]);
  return {
    relationships: [
      { id: "pa-pb", from: "pa", to: "pb" },
      { id: "pb-pc", from: "pb", to: "pc" },
      { id: "pc-pa", from: "pc", to: "pa" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([["pa", 0], ["pb", 2], ["pc", 1]]),
    rowIndexByNode: new Map([["pa", 0], ["pb", 0], ["pc", 1]]),
    canvasWidth: 760,
    canvasHeight: offsetY + 420,
    marginY: 76
  };
}

test("routeEdges reports real, advancing planning progress", () => {
  const reports = [];
  const routes = routeEdges({
    ...progressInput(1090),
    onProgress: (report) => reports.push({ ...report })
  });

  assert.equal(routes.size, 3, "all relationships routed");
  assert.ok(reports.length > 0, "progress was reported");

  const edgePhase = reports.filter((report) => report.label === "Routing edges" && report.total > 0);
  assert.ok(edgePhase.length >= 3, "per-edge completions were reported");
  assert.equal(edgePhase.at(-1).total, 3, "edge total matches routable relationships");
  assert.equal(edgePhase.at(-1).done, 3, "all edges reported done");
  for (const report of edgePhase) {
    assert.ok(report.done <= report.total, "done never exceeds total");
  }

  let previousConsidered = 0;
  for (const report of reports) {
    assert.ok(
      report.routesConsidered >= previousConsidered,
      "routes-considered counter is monotonically nondecreasing"
    );
    previousConsidered = report.routesConsidered;
  }
  assert.ok(reports.at(-1).routesConsidered > 0, "candidate work was counted");

  const labels = new Set(reports.map((report) => report.label));
  assert.ok(labels.size > 1, "quality passes also reported their phase labels");
});

test("planDiagram threads onProgress through to the planner", () => {
  const reports = [];
  const input = progressInput(2310);
  const plan = planDiagram({
    view: { id: "progress-view", lanes: [] },
    relationships: input.relationships,
    visibleNodeIds: input.visibleNodeIds,
    extraNodeRects: input.nodeRects,
    extraLaneIndexByNode: input.laneIndexByNode,
    extraRowIndexByNode: input.rowIndexByNode,
    minCanvasWidth: input.canvasWidth,
    minCanvasHeight: input.canvasHeight,
    onProgress: (report) => reports.push(report)
  });
  assert.ok(plan.routes.size >= 3, "plan routed the relationships");
  assert.ok(reports.some((report) => report.label === "Routing edges"), "progress flowed through planDiagram");
});
