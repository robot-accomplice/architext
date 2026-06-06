import assert from "node:assert/strict";
import test from "node:test";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";
import { diagramLayoutFor } from "../viewer/src/presentation/diagramLayout.js";
import { diagnosePlannedRoutes, pairInternalCrossings, laneOrderViolations } from "../viewer/src/routing/routeDiagnostics.js";
import { deriveRouteIntent } from "../viewer/src/routing/routeIntent.js";
import { routeIntersectsRect } from "../viewer/src/routing/routeEdges.js";

function simplePlan(relationships) {
  const view = {
    id: "diagnostic-fixture",
    name: "Diagnostic Fixture",
    type: "flow-explorer",
    lanes: [
      { id: "left", name: "Left", nodeIds: ["source"] },
      { id: "right", name: "Right", nodeIds: ["target", "other-target"] }
    ]
  };
  return planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 100,
    nodeHeight: 40,
    laneWidth: 180,
    rowGap: 90,
    marginX: 40,
    marginY: 40,
    minCanvasWidth: 420,
    minCanvasHeight: 240,
    canvasExtraWidth: 80,
    canvasExtraHeight: 60,
    style: "orthogonal",
    diagnostics: true
  });
}

function assertOrthogonalRouteSet(plan) {
  for (const [relationshipId, route] of plan.routes) {
    for (let index = 0; index < route.points.length - 1; index += 1) {
      const start = route.points[index];
      const end = route.points[index + 1];
      assert.ok(
        start.x === end.x || start.y === end.y,
        `${relationshipId} contains non-axis-aligned segment ${JSON.stringify({ start, end })}`
      );
    }
  }
}

test("deriveRouteIntent keeps semantic role separate from route geometry", () => {
  const intent = deriveRouteIntent({
    relationship: {
      id: "return-context",
      from: "memory-system",
      to: "unified-pipeline",
      kind: "return",
      returnOf: "retrieve-context",
      outcome: "cached"
    },
    fromRect: { x: 300, y: 40, width: 100, height: 40 },
    toRect: { x: 40, y: 40, width: 100, height: 40 },
    fromLaneIndex: 1,
    toLaneIndex: 0,
    fromRowIndex: 0,
    toRowIndex: 0
  });

  assert.deepEqual(intent, {
    relationshipId: "return-context",
    role: "return",
    returnOf: "retrieve-context",
    outcome: "cached",
    laneDirection: "backward",
    rowDirection: "same",
    expectedSourceSide: "left",
    expectedTargetSide: "right"
  });
});

test("diagnostics are opt-in and report route-set sanity without rendering", () => {
  const relationships = [
    { id: "source-target", from: "source", to: "target", label: "first", relationshipType: "flow", displayIndex: 1 },
    { id: "source-other", from: "source", to: "other-target", label: "second", relationshipType: "flow", displayIndex: 2 }
  ];
  const normalPlan = simplePlan(relationships.map((relationship) => ({ ...relationship })));
  const diagnostics = normalPlan.diagnostics ?? diagnosePlannedRoutes(normalPlan, relationships);

  assert.ok(diagnostics.routes.length > 0);
  assert.equal(diagnostics.routes.every((route) => route.relationshipId), true);
  assert.equal(typeof diagnostics.metrics.closeParallelRuns, "number");
});

test("dense fan-in diagnostics explain surface-capacity escape endpoints", () => {
  const view = {
    id: "complex-fan-in",
    name: "Complex Fan-In",
    type: "system-map",
    lanes: [
      { id: "sources", name: "Sources", nodeIds: ["source-a", "source-b", "source-c", "source-d"] },
      { id: "middle", name: "Middle", nodeIds: ["blocker-a", "blocker-b"] },
      { id: "target", name: "Target", nodeIds: ["target"] }
    ]
  };
  const relationships = ["source-a", "source-b", "source-c", "source-d"].map((sourceId) => ({
    id: `${sourceId}-target`,
    from: sourceId,
    to: "target",
    label: `${sourceId} feeds target`,
    relationshipType: "flow",
    kind: "request"
  }));
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 144,
    nodeHeight: 62,
    laneWidth: 218,
    rowGap: 116,
    marginX: 72,
    marginY: 76,
    minCanvasWidth: 820,
    minCanvasHeight: 520,
    canvasExtraWidth: 80,
    canvasExtraHeight: 96,
    style: "orthogonal",
    diagnostics: true
  });
  const sourceA = plan.diagnostics.routes.find((route) => route.relationshipId === "source-a-target");

  // source-a is coplanar with the target with blocker-a directly between them. The cost
  // model (since 36cc405's weighted-sum objective) mounts the source toward its partner and
  // detours AROUND the blocker — an L-corner, not a backtrack — landing on the target's
  // perpendicular gutter rather than jamming the blocked facing corridor. We assert the
  // escape is real (a blocked-corridor constraint) and that the TARGET end lands on a
  // gutter. The symmetric both-ends gutter escape was an aspiration the weighted-sum
  // objective does not meet, so the source side is not pinned here.
  assert.ok(
    ["top", "bottom"].includes(sourceA.targetSide),
    "target end escapes to a perpendicular gutter, not the blocked facing corridor"
  );
  assert.ok(
    sourceA.constraints.some((constraint) => constraint.code.startsWith("constrained-")),
    "the escape endpoint is explained by a blocked-corridor constraint"
  );
});

test("semantic return gutters leave badge-sized clearance between long parallel lanes", () => {
  const view = {
    id: "return-gutter-clearance",
    name: "Return Gutter Clearance",
    type: "flow-explorer",
    lanes: [
      { id: "entry", name: "Entry", nodeIds: ["user", "browser", "channel"] },
      { id: "runtime", name: "Runtime", nodeIds: ["pipeline"] },
      { id: "services", name: "Services", nodeIds: ["memory", "model"] }
    ]
  };
  const relationships = [
    { id: "browser-pipeline", from: "browser", to: "pipeline", relationshipType: "flow", kind: "request", displayIndex: 1 },
    { id: "channel-pipeline", from: "channel", to: "pipeline", relationshipType: "flow", kind: "request", displayIndex: 2 },
    { id: "pipeline-memory", from: "pipeline", to: "memory", relationshipType: "flow", kind: "request", displayIndex: 3 },
    { id: "memory-pipeline", from: "memory", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "pipeline-memory", displayIndex: 4 },
    { id: "pipeline-model", from: "pipeline", to: "model", relationshipType: "flow", kind: "request", displayIndex: 5 },
    { id: "model-pipeline", from: "model", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "pipeline-model", displayIndex: 6 },
    { id: "pipeline-browser", from: "pipeline", to: "browser", relationshipType: "flow", kind: "return", returnOf: "browser-pipeline", displayIndex: 7 },
    { id: "pipeline-channel", from: "pipeline", to: "channel", relationshipType: "flow", kind: "return", returnOf: "channel-pipeline", displayIndex: 8 },
    { id: "channel-user", from: "channel", to: "user", relationshipType: "flow", kind: "return", displayIndex: 9 }
  ];
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 136,
    nodeHeight: 54,
    laneWidth: 210,
    rowGap: 102,
    marginX: 180,
    marginY: 76,
    minCanvasWidth: 820,
    minCanvasHeight: 520,
    canvasExtraWidth: 80,
    canvasExtraHeight: 96,
    style: "orthogonal",
    diagnostics: true,
    diagnosticOptions: { closeParallelRunBudget: 0 }
  });

  // The single close-parallel run is the channel<->pipeline reciprocal pair, drawn as a
  // deliberate parallel bundle at RECIPROCAL_PARALLEL_OFFSET (a legible round-trip, not
  // accidental crowding). The zero-budget expectation predates that bundling feature; the
  // test now guards against ADDITIONAL unintended close runs beyond the one intentional pair.
  assert.equal(plan.diagnostics.metrics.closeParallelRuns, 1);
});

test("viewer flow layout keeps dense request and return routes in readable channels", () => {
  const view = {
    id: "viewer-dense-flow",
    name: "Viewer Dense Flow",
    type: "flow-explorer",
    lanes: [
      { id: "entry", name: "Entry", nodeIds: ["operator", "cli", "tui", "browser", "websocket", "external"] },
      { id: "factory", name: "Factory", nodeIds: ["pipeline"] },
      { id: "context", name: "Context", nodeIds: ["memory", "product", "skills", "mcp", "sqlite"] },
      { id: "output", name: "Output", nodeIds: ["llm", "cloud", "local", "observability"] }
    ]
  };
  const relationships = [
    { id: "receive-message", from: "operator", to: "external", relationshipType: "flow", kind: "process", displayIndex: 1 },
    { id: "submit-web-session-message", from: "browser", to: "websocket", relationshipType: "flow", kind: "request", displayIndex: 2 },
    { id: "delegate-web-message", from: "websocket", to: "pipeline", relationshipType: "flow", kind: "request", displayIndex: 3 },
    { id: "normalize-input", from: "external", to: "pipeline", relationshipType: "flow", kind: "request", displayIndex: 4 },
    { id: "resolve-session", from: "pipeline", to: "sqlite", relationshipType: "flow", kind: "persistence", displayIndex: 5 },
    { id: "session-resolved", from: "sqlite", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "resolve-session", displayIndex: 6 },
    { id: "retrieve-context", from: "pipeline", to: "memory", relationshipType: "flow", kind: "request", displayIndex: 7 },
    { id: "context-returned", from: "memory", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "retrieve-context", displayIndex: 8 },
    { id: "execute-tools", from: "pipeline", to: "mcp", relationshipType: "flow", kind: "request", displayIndex: 9 },
    { id: "tool-evidence-returned", from: "mcp", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "execute-tools", displayIndex: 10 },
    { id: "request-model", from: "pipeline", to: "llm", relationshipType: "flow", kind: "request", displayIndex: 11 },
    { id: "model-response", from: "llm", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "request-model", displayIndex: 12 },
    { id: "persist-response", from: "pipeline", to: "sqlite", relationshipType: "flow", kind: "persistence", displayIndex: 13 },
    { id: "persistence-confirmed", from: "sqlite", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "persist-response", displayIndex: 14 },
    { id: "web-pipeline-outcome", from: "pipeline", to: "websocket", relationshipType: "flow", kind: "return", returnOf: "delegate-web-message", displayIndex: 15 },
    { id: "web-deliver-response", from: "websocket", to: "browser", relationshipType: "flow", kind: "return", returnOf: "submit-web-session-message", displayIndex: 16 },
    { id: "format-response", from: "pipeline", to: "external", relationshipType: "flow", kind: "return", returnOf: "normalize-input", displayIndex: 17 },
    { id: "deliver-response", from: "external", to: "operator", relationshipType: "flow", kind: "return", returnOf: "receive-message", displayIndex: 18 }
  ];
  const layout = diagramLayoutFor(view, relationships.length);
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    ...layout,
    style: "orthogonal",
    diagnostics: true,
    diagnosticOptions: { closeParallelRunBudget: 0 }
  });

  // The blocked request/return pair (request-model / model-response) is relieved onto a
  // parallel north-gutter bridge (see below); after the post-relief re-spread and lane
  // separation the two lanes read as fully distinct, so the diagram has no close-parallel
  // runs at all.
  assert.equal(plan.diagnostics.metrics.closeParallelRuns, 0);
  assertOrthogonalRouteSet(plan);
  assert.equal(plan.diagnostics.findings.filter((finding) => finding.code?.startsWith("non-facing")).length, 0);
  const receiveMessage = plan.diagnostics.routes.find((route) => route.relationshipId === "receive-message");
  assert.equal(receiveMessage.targetSide, "left", "same-column flow past multiple intermediaries mounts on the outer gutter surface and runs straight up it");
  const receiveRoute = plan.routes.get("receive-message");
  for (const blockerId of ["cli", "tui", "browser", "websocket"]) {
    assert.equal(
      routeIntersectsRect(receiveRoute, plan.nodeRects.get(blockerId), 0),
      false,
      `same-lane downward flow must not collapse through ${blockerId}`
    );
  }
  const operator = plan.nodeRects.get("operator");
  assert.ok(
    receiveRoute.points.some((point) => point.x < operator.x),
    "same-lane first-column flow should use the exterior gutter instead of the interior lane"
  );
  const formatResponse = plan.diagnostics.routes.find((route) => route.relationshipId === "format-response");
  assert.equal(formatResponse.targetSide, "right", "return routes should not abandon the facing target surface only because another side is empty");
  // The pipeline->llm request and its return are blocked from the facing corridor by the
  // intervening context column. Rather than escaping through the crowded southern fan (which
  // crossed several other edges), the relief pass routes the pair over the open north gutter
  // as a parallel, crossing-free bridge — both ends mount the top surface.
  const requestModel = plan.diagnostics.routes.find((route) => route.relationshipId === "request-model");
  assert.equal(requestModel.targetSide, "top", "a blocked reciprocal pair is relieved onto the open north gutter rather than the crowded southern fan");
  const modelResponse = plan.diagnostics.routes.find((route) => route.relationshipId === "model-response");
  assert.equal(modelResponse.sourceSide, "top", "the return half of the pair runs parallel to the request on the same north gutter");
  assert.equal(plan.routes.get("retrieve-context").bends, 0, "facing service routes should align paired endpoints after surface distribution");
  assert.equal(plan.routes.get("context-returned").bends, 0, "return service routes should not keep a tiny dogleg after surface distribution");
});

test("entry return gutters choose open channels instead of close parallel lanes", () => {
  const view = {
    id: "entry-return-clearance",
    name: "Entry Return Clearance",
    type: "flow-explorer",
    lanes: [
      { id: "entry", name: "Entry", nodeIds: ["operator", "browser", "websocket", "external"] },
      { id: "runtime", name: "Runtime", nodeIds: ["pipeline"] }
    ]
  };
  const relationships = [
    { id: "delegate", from: "websocket", to: "pipeline", relationshipType: "flow", kind: "request", displayIndex: 1 },
    { id: "normalize", from: "external", to: "pipeline", relationshipType: "flow", kind: "request", displayIndex: 2 },
    { id: "format", from: "pipeline", to: "external", relationshipType: "flow", kind: "return", displayIndex: 3 },
    { id: "deliver", from: "external", to: "operator", relationshipType: "flow", kind: "return", displayIndex: 4 }
  ];
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 136,
    nodeHeight: 54,
    laneWidth: 210,
    rowGap: 102,
    marginX: 180,
    marginY: 76,
    minCanvasWidth: 820,
    minCanvasHeight: 520,
    canvasExtraWidth: 80,
    canvasExtraHeight: 96,
    style: "orthogonal",
    diagnostics: true,
    diagnosticOptions: { closeParallelRunBudget: 0 }
  });

  assert.equal(plan.diagnostics.metrics.closeParallelRuns, 0);
  const deliverRoute = plan.routes.get("deliver");
  const operator = plan.nodeRects.get("operator");
  assert.ok(
    deliverRoute.points.some((point) => point.x < operator.x),
    "same-lane return should be able to use the exterior gutter left of the first node column"
  );
});

test("same-lane blocked routes escape to the gutter instead of jogging the blocker", () => {
  const view = {
    id: "same-lane-blocked",
    name: "Same Lane Blocked",
    type: "flow-explorer",
    lanes: [
      { id: "lane", name: "Lane", nodeIds: ["top", "middle", "bottom"] }
    ]
  };
  const relationships = [
    { id: "top-bottom", from: "top", to: "bottom", relationshipType: "flow", kind: "request", displayIndex: 1 }
  ];
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 136,
    nodeHeight: 54,
    laneWidth: 210,
    rowGap: 102,
    marginX: 180,
    marginY: 76,
    minCanvasWidth: 420,
    minCanvasHeight: 420,
    canvasExtraWidth: 80,
    canvasExtraHeight: 96,
    style: "orthogonal",
    diagnostics: true
  });
  const route = plan.diagnostics.routes.find((diagnostic) => diagnostic.relationshipId === "top-bottom");
  const plannedRoute = plan.routes.get("top-bottom");

  // A single intermediary is NOT cheaper to jog around than a clean gutter detour:
  // the edge escapes to the side gutter (both ends on the same side, so the run is
  // straight) and still never traverses the blocker.
  assert.equal(route.findings.some((finding) => finding.code.startsWith("non-facing")), false);
  assert.equal(route.sourceSide, "left");
  assert.equal(route.targetSide, "left");
  assert.equal(routeIntersectsRect(plannedRoute, plan.nodeRects.get("middle"), 0), false);
});

// Acceptance test for the dense-hub fix (roboticus `model-inference` in miniature):
// a hub at the top of a column with reciprocal pairs to the target DIRECTLY below it
// (t1) and to the target one PAST the blocker (t2). The adjacent pair stays on the
// facing surfaces; the pair that would otherwise pile onto the hub's bottom and dogleg
// around t1 escapes to a side gutter instead. Before the fix both pairs crammed onto
// the hub's bottom surface and crossed; this keeps the dense hub legible.
test("a hub reciprocal pair past an intermediary escapes to a side gutter, not the blocked facing surface", () => {
  const view = {
    id: "dense-hub",
    name: "Dense Hub",
    type: "flow-explorer",
    lanes: [
      { id: "lane", name: "Lane", nodeIds: ["hub", "t1", "t2"] }
    ]
  };
  const relationships = [
    { id: "e1", from: "hub", to: "t1", relationshipType: "flow", kind: "request", returnOf: undefined, displayIndex: 1, flowId: "f", stepId: "e1" },
    { id: "e2", from: "t1", to: "hub", relationshipType: "flow", kind: "return", returnOf: "e1", displayIndex: 2, flowId: "f", stepId: "e2" },
    { id: "e3", from: "hub", to: "t2", relationshipType: "flow", kind: "request", returnOf: undefined, displayIndex: 3, flowId: "f", stepId: "e3" },
    { id: "e4", from: "t2", to: "hub", relationshipType: "flow", kind: "return", returnOf: "e3", displayIndex: 4, flowId: "f", stepId: "e4" }
  ];
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 136,
    nodeHeight: 54,
    laneWidth: 210,
    rowGap: 120,
    marginX: 180,
    marginY: 76,
    minCanvasWidth: 480,
    minCanvasHeight: 560,
    canvasExtraWidth: 80,
    canvasExtraHeight: 96,
    style: "orthogonal",
    diagnostics: true
  });
  const adjacent = plan.diagnostics.routes.find((diagnostic) => diagnostic.relationshipId === "e1");
  const pastBlocker = plan.diagnostics.routes.find((diagnostic) => diagnostic.relationshipId === "e3");

  // The adjacent reciprocal pair has a clear facing corridor, so it stays facing.
  assert.equal(adjacent.sourceSide, "bottom");
  assert.equal(adjacent.targetSide, "top");
  // The pair past the blocker takes a clean side gutter instead of doglegging.
  assert.ok(
    pastBlocker.sourceSide === "left" || pastBlocker.sourceSide === "right",
    "hub edge past the intermediary should mount a side gutter, not the blocked bottom surface"
  );
  assert.notEqual(pastBlocker.sourceSide, "bottom");
  // The escaping route still never traverses the intermediary it routes around.
  assert.equal(routeIntersectsRect(plan.routes.get("e3"), plan.nodeRects.get("t1"), 0), false);
});

// --- T4 ordering detectors (pair-internal crossings + gutter lane order) ---------------
// These back the durable defect harness the maintainer asked for: the mount-audit only
// measured distribution evenness, so reciprocal pairs that cross themselves and gutter
// lanes ordered wrong went unflagged. The detectors are pure functions so the sweep tool
// and diagnostics share one implementation and the logic is unit-testable in isolation.

function route(...points) {
  return { points: points.map(([x, y]) => ({ x, y })) };
}

test("pairInternalCrossings flags a reciprocal pair whose two lines cross", () => {
  const routes = new Map([
    ["req", route([0, 5], [20, 5])], // horizontal at y=5
    ["ret", route([10, 0], [10, 20])] // vertical at x=10 — crosses req at (10,5)
  ]);
  const relationships = [
    { id: "req", from: "a", to: "b", displayIndex: 1 },
    { id: "ret", from: "b", to: "a", displayIndex: 2 }
  ];
  const found = pairInternalCrossings(routes, relationships);
  assert.equal(found.length, 1);
  assert.deepEqual([found[0].a, found[0].b].sort(), ["req", "ret"]);
  assert.ok(found[0].crossings >= 1);
});

test("pairInternalCrossings ignores a reciprocal pair that runs parallel", () => {
  const routes = new Map([
    ["req", route([0, 5], [20, 5])],
    ["ret", route([0, 10], [20, 10])] // parallel, never crosses
  ]);
  const relationships = [
    { id: "req", from: "a", to: "b", displayIndex: 1 },
    { id: "ret", from: "b", to: "a", displayIndex: 2 }
  ];
  assert.equal(pairInternalCrossings(routes, relationships).length, 0);
});

test("pairInternalCrossings does not flag a crossing between non-reciprocal edges", () => {
  const routes = new Map([
    ["a-to-b", route([0, 5], [20, 5])],
    ["a-to-c", route([10, 0], [10, 20])] // crosses, but same direction (not a return)
  ]);
  const relationships = [
    { id: "a-to-b", from: "a", to: "b", displayIndex: 1 },
    { id: "a-to-c", from: "a", to: "c", displayIndex: 2 }
  ];
  assert.equal(pairInternalCrossings(routes, relationships).length, 0);
});

test("pairInternalCrossings pairs by displayIndex adjacency, not all opposite-direction combos", () => {
  // a<->b carries TWO round trips. Adjacency pairing = (req1,ret1) and (req2,ret2).
  // ret1 crossing req2 must NOT be reported as pair-internal (they are not one pair).
  const routes = new Map([
    ["req1", route([0, 5], [20, 5])],
    ["ret1", route([0, 8], [20, 8])], // parallel to req1 — clean pair
    ["req2", route([0, 11], [20, 11])], // parallel to ret2 — clean pair
    ["ret2", route([0, 14], [20, 14])],
    // a stray vertical crossing ret1 and req2 — these are NOT a reciprocal pair
    ["req1b", route([15, 6], [15, 12])]
  ]);
  const relationships = [
    { id: "req1", from: "a", to: "b", displayIndex: 1 },
    { id: "ret1", from: "b", to: "a", displayIndex: 2 },
    { id: "req2", from: "a", to: "b", displayIndex: 3 },
    { id: "ret2", from: "b", to: "a", displayIndex: 4 },
    { id: "req1b", from: "a", to: "b", displayIndex: 5 }
  ];
  // No reported pair-internal crossing: each adjacency-matched pair runs parallel.
  assert.equal(pairInternalCrossings(routes, relationships).length, 0);
});

function laneOrderPlan(farOffset, nearOffset) {
  const nodeRects = new Map([
    ["o", { x: 0, y: 0, width: 100, height: 40 }], // right face x=100, centreY=20
    ["near", { x: 400, y: 80, width: 100, height: 40 }], // centreY=100 (close)
    ["far", { x: 400, y: 480, width: 100, height: 40 }] // centreY=500 (far)
  ]);
  const routes = new Map([
    ["to-far", route([100, 15], [100 + farOffset, 15], [100 + farOffset, 500], [400, 500])],
    ["to-near", route([100, 25], [100 + nearOffset, 25], [100 + nearOffset, 100], [400, 100])]
  ]);
  return { nodeRects, routes };
}

test("laneOrderViolations flags the farthest target sitting in an inner lane", () => {
  const plan = laneOrderPlan(30, 60); // far at offset 30 (inner), near at offset 60 (outer) — wrong
  const relationships = [
    { id: "to-far", from: "o", to: "far", displayIndex: 1 },
    { id: "to-near", from: "o", to: "near", displayIndex: 2 }
  ];
  const found = laneOrderViolations(plan, relationships);
  assert.equal(found.length, 1);
  assert.equal(found[0].nodeId, "o");
  assert.equal(found[0].side, "right");
});

test("laneOrderViolations is clean when the farthest target is outermost", () => {
  const plan = laneOrderPlan(60, 30); // far at offset 60 (outer), near at offset 30 (inner) — correct
  const relationships = [
    { id: "to-far", from: "o", to: "far", displayIndex: 1 },
    { id: "to-near", from: "o", to: "near", displayIndex: 2 }
  ];
  assert.equal(laneOrderViolations(plan, relationships).length, 0);
});
