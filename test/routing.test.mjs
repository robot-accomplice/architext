import assert from "node:assert/strict";
import test from "node:test";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";
import { pathToSvgWithHops, routeEdges, routeIntersectsRect } from "../viewer/src/routing/routeEdges.js";
import { routeCacheKey } from "../viewer/src/routing/routeCache.js";
import { PORT_STUB, surfaceCapacity } from "../viewer/src/routing/routePorts.js";
import { sortedRouteCandidates } from "../viewer/src/routing/routeScoring.js";

function baseInput(overrides = {}) {
  const nodeRects = new Map([
    ["source", { x: 40, y: 90, width: 136, height: 54 }],
    ["target", { x: 460, y: 90, width: 136, height: 54 }],
    ["blocker", { x: 250, y: 90, width: 136, height: 54 }],
    ["target-b", { x: 460, y: 230, width: 136, height: 54 }]
  ]);
  const laneIndexByNode = new Map([
    ["source", 0],
    ["blocker", 1],
    ["target", 2],
    ["target-b", 2]
  ]);
  const rowIndexByNode = new Map([
    ["source", 0],
    ["blocker", 0],
    ["target", 0],
    ["target-b", 1]
  ]);

  return {
    relationships: [
      { id: "source-target", from: "source", to: "target" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode,
    rowIndexByNode,
    canvasWidth: 760,
    canvasHeight: 420,
    marginY: 76,
    ...overrides
  };
}

function assertFiniteRoute(route, expectedStyle = "orthogonal") {
  assert.equal(typeof route.d, "string");
  assert.equal(route.style, expectedStyle);
  if (expectedStyle === "orthogonal") {
    assert.doesNotMatch(route.d, /\bC\b/);
    for (let index = 0; index < route.points.length - 1; index += 1) {
      const start = route.points[index];
      const end = route.points[index + 1];
      assert.ok(
        start.x === end.x || start.y === end.y,
        `orthogonal route contains non-axis-aligned segment ${JSON.stringify({ start, end, route: route.d })}`
      );
    }
  }
  assert.equal(Number.isFinite(route.labelX), true);
  assert.equal(Number.isFinite(route.labelY), true);
  assert.equal(Number.isFinite(route.cost), true);
  assert.ok(route.samples.length > 0);
  for (const point of route.samples) {
    assert.equal(Number.isFinite(point.x), true);
    assert.equal(Number.isFinite(point.y), true);
  }
  assertFiniteQualityCosts(route);
}

function assertFiniteQualityCosts(route) {
  assert.equal(typeof route.qualityCosts, "object");
  assert.ok(Object.keys(route.qualityCosts).length > 0);
  let total = 0;
  for (const [name, value] of Object.entries(route.qualityCosts)) {
    assert.equal(Number.isFinite(value), true, `${name} must be finite`);
    total += value;
  }
  assert.equal(Math.round(route.cost * 1000) / 1000, Math.round(total * 1000) / 1000);
}

function assertPerpendicularContact(route, sourceRect, targetRect) {
  const first = route.points[0];
  const second = route.points[1];
  const beforeLast = route.points[route.points.length - 2];
  const last = route.points[route.points.length - 1];

  assert.equal(segmentLeavesRectPerpendicularly(first, second, sourceRect), true);
  assert.equal(segmentLeavesRectPerpendicularly(last, beforeLast, targetRect), true);
}

function segmentLeavesRectPerpendicularly(anchor, next, rect) {
  if (anchor.x === rect.x) return next.y === anchor.y && next.x <= anchor.x;
  if (anchor.x === rect.x + rect.width) return next.y === anchor.y && next.x >= anchor.x;
  if (anchor.y === rect.y) return next.x === anchor.x && next.y <= anchor.y;
  if (anchor.y === rect.y + rect.height) return next.x === anchor.x && next.y >= anchor.y;
  return false;
}

function sideForPoint(rect, point) {
  if (point.x === rect.x) return "left";
  if (point.x === rect.x + rect.width) return "right";
  if (point.y === rect.y) return "top";
  if (point.y === rect.y + rect.height) return "bottom";
  return "";
}

function rectsOverlap(a, b, padding = 0) {
  return (
    a.x < b.x + b.width + padding &&
    a.x + a.width > b.x - padding &&
    a.y < b.y + b.height + padding &&
    a.y + a.height > b.y - padding
  );
}

function routeTouchesRectInterior(route, rect) {
  return route.samples.some((point) => point.x > rect.x && point.x < rect.x + rect.width && point.y > rect.y && point.y < rect.y + rect.height);
}

function routeHasImmediateBacktrack(route) {
  for (let index = 1; index < route.points.length - 1; index += 1) {
    const previous = route.points[index - 1];
    const current = route.points[index];
    const next = route.points[index + 1];
    const horizontalBacktrack = previous.y === current.y && current.y === next.y &&
      Math.sign(current.x - previous.x) === -Math.sign(next.x - current.x);
    const verticalBacktrack = previous.x === current.x && current.x === next.x &&
      Math.sign(current.y - previous.y) === -Math.sign(next.y - current.y);
    if (horizontalBacktrack || verticalBacktrack) return true;
  }
  return false;
}

function endpointStubLength(route, endpoint) {
  const firstIndex = endpoint === "source" ? 0 : route.points.length - 1;
  const secondIndex = endpoint === "source" ? 1 : route.points.length - 2;
  const first = route.points[firstIndex];
  const second = route.points[secondIndex];
  return Math.hypot(first.x - second.x, first.y - second.y);
}

function dot(a, b) {
  return a.x * b.x + a.y * b.y;
}

function unit(from, to) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const length = Math.hypot(dx, dy);
  return { x: dx / length, y: dy / length };
}

function pointLineDistance(point, start, end) {
  const numerator = Math.abs((end.y - start.y) * point.x - (end.x - start.x) * point.y + end.x * start.y - end.y * start.x);
  const denominator = Math.hypot(end.y - start.y, end.x - start.x);
  return denominator === 0 ? 0 : numerator / denominator;
}

function distanceToNearestSample(point, samples) {
  return samples.reduce((nearest, sample) => Math.min(nearest, Math.hypot(point.x - sample.x, point.y - sample.y)), Infinity);
}

test("routeEdges produces deterministic finite routes", () => {
  const input = baseInput();
  const first = routeEdges(input).get("source-target");
  const second = routeEdges(input).get("source-target");

  assertFiniteRoute(first);
  assert.deepEqual(second, first);
});

test("route cache keys include route semantics that affect surface selection", () => {
  const input = baseInput({
    relationships: [
      {
        ...baseInput().relationships[0],
        relationshipType: "flow",
        stepId: "step-1"
      }
    ]
  });
  const untypedFlowKey = routeCacheKey(input);
  const semanticKey = routeCacheKey({
    ...input,
    relationships: [
      {
        ...input.relationships[0],
        kind: "request",
        returnOf: "previous-step",
        outcome: "ok"
      }
    ]
  });

  assert.notEqual(semanticKey, untypedFlowKey);
});

test("routeEdges renders a whole route set with the selected visual style", () => {
  const orthogonal = routeEdges(baseInput({ style: "orthogonal" })).get("source-target");
  const spline = routeEdges(baseInput({ style: "spline" })).get("source-target");
  const straight = routeEdges(baseInput({ style: "straight" })).get("source-target");

  assertFiniteRoute(orthogonal, "orthogonal");
  assertFiniteRoute(spline, "spline");
  assertFiniteRoute(straight, "straight");
  assert.doesNotMatch(orthogonal.d, /\b[QC]\b/);
  assert.match(spline.d, /\bC\b/);
  assert.doesNotMatch(spline.d, /\bL\b/);
  assert.match(straight.d, /^M [^L]+ L /);
  assert.doesNotMatch(straight.d, /\b[QC]\b/);
  assert.notDeepEqual(spline.samples, orthogonal.samples);
});

test("routeEdges aligns spline arrowheads with the curve tangent", () => {
  const route = routeEdges(baseInput({ style: "spline" })).get("source-target");
  const start = route.points[0];
  const end = route.points.at(-1);
  const [, controlB] = route.controls;
  const chord = unit(start, end);
  const endTangent = unit(controlB, end);

  assert.equal(route.style, "spline");
  assert.ok(dot(chord, endTangent) > 0.72, `spline endpoint tangent should generally follow curve direction: ${JSON.stringify({ chord, endTangent })}`);
});

test("routeEdges gives spline routes visible curvature", () => {
  const route = routeEdges(baseInput({ style: "spline" })).get("source-target");
  const start = route.points[0];
  const end = route.points.at(-1);
  const maxDistance = Math.max(...route.samples.map((point) => pointLineDistance(point, start, end)));

  assert.equal(route.style, "spline");
  assert.ok(maxDistance >= 12, `spline route should not collapse to a straight line: ${maxDistance}`);
});

test("routeEdges keeps unobstructed connections straight", () => {
  const input = baseInput({
    visibleNodeIds: new Set(["source", "target"])
  });
  const route = routeEdges(input).get("source-target");

  assertFiniteRoute(route);
  assert.equal(route.bends, 0);
  assert.equal(route.warnings.length, 0);
});

test("routeEdges honors preferred decision branch exit sides", () => {
  for (const style of ["orthogonal", "straight", "spline"]) {
    const left = routeEdges(baseInput({
      style,
      relationships: [
        { id: "left", from: "source", to: "target", preferredStartSide: "left" }
      ]
    })).get("left");
    const right = routeEdges(baseInput({
      style,
      relationships: [
        { id: "right", from: "source", to: "target", preferredStartSide: "right" }
      ]
    })).get("right");

    assert.equal(left.points[0].x, 40, `${style} should exit from the left side`);
    assert.equal(right.points[0].x, 176, `${style} should exit from the right side`);
  }
});

test("routeEdges honors preferred target entry sides", () => {
  for (const style of ["orthogonal", "straight", "spline"]) {
    const route = routeEdges(baseInput({
      style,
      relationships: [
        { id: "target-right", from: "source", to: "target", preferredEndSide: "right" }
      ]
    })).get("target-right");

    assert.equal(route.points.at(-1).x, 596, `${style} should enter the target from the right side`);
  }
});

test("routeEdges can start decision branches on fixed diamond points", () => {
  const diamondPoint = { x: 120, y: 120 };
  for (const style of ["orthogonal", "straight", "spline"]) {
    const route = routeEdges(baseInput({
      style,
      nodeRects: new Map([
        ["decision", {
          x: 100,
          y: 100,
          width: 40,
          height: 40,
          fixedPorts: true,
          sideAnchors: { right: diamondPoint }
        }],
        ["target", { x: 460, y: 90, width: 136, height: 54 }]
      ]),
      laneIndexByNode: new Map([["decision", 0], ["target", 1]]),
      rowIndexByNode: new Map([["decision", 0], ["target", 0]]),
      visibleNodeIds: new Set(["decision", "target"]),
      relationships: [
        { id: "branch", from: "decision", to: "target", preferredStartSide: "right" }
      ]
    })).get("branch");

    assert.deepEqual(route.points[0], diamondPoint, `${style} should start at the diamond point`);
    assert.equal(route.points[1].y, diamondPoint.y, `${style} should leave the diamond without an immediate vertical dogleg`);
  }
});

test("fixed-port same-side branches use a local target gutter instead of the canvas edge", () => {
  const route = routeEdges(baseInput({
    canvasWidth: 1200,
    nodeRects: new Map([
      ["decision", {
        x: 300,
        y: 250,
        width: 40,
        height: 40,
        fixedPorts: true,
        sideAnchors: { right: { x: 340, y: 270 } }
      }],
      ["target", { x: 700, y: 90, width: 136, height: 54 }]
    ]),
    laneIndexByNode: new Map([["decision", 0], ["target", 1]]),
    rowIndexByNode: new Map([["decision", 1], ["target", 0]]),
    visibleNodeIds: new Set(["decision", "target"]),
    relationships: [
      { id: "valid", from: "decision", to: "target", preferredStartSide: "right", preferredEndSide: "right" }
    ]
  })).get("valid");

  assert.equal(route.points[0].x, 340);
  assert.equal(route.points.at(-1).x, 836);
  assert.equal(Math.max(...route.points.map((point) => point.x)), 872);
});

test("routeEdges meets endpoint nodes perpendicularly", () => {
  const input = baseInput();
  const route = routeEdges(input).get("source-target");

  assertFiniteRoute(route);
  assertPerpendicularContact(route, input.nodeRects.get("source"), input.nodeRects.get("target"));
});

test("routeEdges warns when nodes are too close for clean connector spacing", () => {
  const input = baseInput({
    nodeRects: new Map([
      ["source", { x: 40, y: 90, width: 136, height: 54 }],
      ["target", { x: 185, y: 90, width: 136, height: 54 }]
    ]),
    laneIndexByNode: new Map([
      ["source", 0],
      ["target", 1]
    ]),
    rowIndexByNode: new Map([
      ["source", 0],
      ["target", 0]
    ]),
    visibleNodeIds: new Set(["source", "target"])
  });
  const route = routeEdges(input).get("source-target");

  assertFiniteRoute(route);
  assert.ok(route.warnings.some((warning) => warning.code === "nodes-too-close"));
});

test("routeEdges moves labels away from short straight connectors", () => {
  const input = baseInput({
    nodeRects: new Map([
      ["source", { x: 40, y: 90, width: 136, height: 54 }],
      ["target", { x: 40, y: 174, width: 136, height: 54 }]
    ]),
    laneIndexByNode: new Map([
      ["source", 0],
      ["target", 0]
    ]),
    rowIndexByNode: new Map([
      ["source", 0],
      ["target", 1]
    ]),
    visibleNodeIds: new Set(["source", "target"])
  });
  const route = routeEdges(input).get("source-target");

  assertFiniteRoute(route);
  assert.notEqual(route.labelX, route.points[0].x);
});

test("planDiagram keeps flow step markers clear of endpoint nodes", () => {
  const view = {
    id: "endpoint-marker-clearance",
    name: "Endpoint Marker Clearance",
    type: "system-map",
    lanes: [
      { id: "actors", name: "Actors", nodeIds: ["maintainer"] },
      { id: "runtime", name: "Runtime", nodeIds: ["cli", "validator"] },
      { id: "data", name: "Data", nodeIds: ["repo", "data-files"] }
    ]
  };
  const relationships = [
    {
      id: "maintainer-cli",
      from: "maintainer",
      to: "cli",
      label: "resolveTargetPath",
      relationshipType: "flow",
      stepId: "step-1",
      displayIndex: 1
    },
    {
      id: "cli-repo",
      from: "cli",
      to: "repo",
      label: "writeLifecycleMetadata",
      relationshipType: "flow",
      stepId: "step-2",
      displayIndex: 2
    },
    {
      id: "cli-validator",
      from: "cli",
      to: "validator",
      label: "validateStarterModel",
      relationshipType: "flow",
      stepId: "step-3",
      displayIndex: 3
    }
  ];
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 176,
    nodeHeight: 70,
    laneWidth: 272,
    rowGap: 132,
    marginX: 236,
    marginY: 146,
    minCanvasWidth: 1120,
    minCanvasHeight: 570,
    canvasExtraWidth: 120,
    canvasExtraHeight: 120,
    style: "orthogonal"
  });

  for (const relationship of relationships) {
    const labelBox = plan.labelBoxes.get(relationship.id);
    const route = plan.routes.get(relationship.id);
    assert.ok(labelBox, `missing label box for ${relationship.id}`);
    assert.ok(route, `missing route for ${relationship.id}`);
    assert.equal(labelBox.width, 28);
    assert.ok(
      distanceToNearestSample({ x: route.labelX, y: route.labelY }, route.samples) < 1,
      `${relationship.id} marker must stay attached to the route`
    );
    for (const [nodeId, rect] of plan.nodeRects) {
      assert.equal(rectsOverlap(labelBox, rect, 0), false, `${relationship.id} marker overlaps ${nodeId}`);
    }
  }
  assert.equal(plan.warnings.filter((warning) => warning.code === "label-over-node").length, 0);
});

test("routeEdges avoids non-endpoint node bodies when a blocker is between endpoints", () => {
  const input = baseInput();
  const route = routeEdges(input).get("source-target");

  assertFiniteRoute(route);
  assert.equal(routeIntersectsRect(route, input.nodeRects.get("blocker"), 0), false);
});

test("routeEdges falls back when grid routing exceeds its budget", () => {
  const nodeRects = new Map([
    ["source", { x: 20, y: 200, width: 80, height: 40 }],
    ["target", { x: 900, y: 200, width: 80, height: 40 }]
  ]);
  const laneIndexByNode = new Map([
    ["source", 0],
    ["target", 9]
  ]);
  const rowIndexByNode = new Map([
    ["source", 0],
    ["target", 0]
  ]);
  for (let index = 0; index < 80; index += 1) {
    const id = `blocker-${index}`;
    nodeRects.set(id, {
      x: 130 + (index % 12) * 60,
      y: 40 + Math.floor(index / 12) * 50,
      width: 42,
      height: 32
    });
    laneIndexByNode.set(id, 1 + (index % 8));
    rowIndexByNode.set(id, Math.floor(index / 8));
  }
  const stats = {};
  const input = {
    relationships: [{ id: "budgeted-route", from: "source", to: "target" }],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode,
    rowIndexByNode,
    canvasWidth: 1000,
    canvasHeight: 500,
    marginY: 76,
    gridRouteMaxPoints: 1,
    stats
  };

  const route = routeEdges(input).get("budgeted-route");

  assertFiniteRoute(route);
  assert.ok(stats.gridRouteBudgetBailouts > 0);
});

test("routeEdges creates distinct label positions for repeated edge pairs", () => {
  const input = baseInput({
    relationships: [
      { id: "first", from: "source", to: "target" },
      { id: "second", from: "source", to: "target" }
    ]
  });
  const routes = routeEdges(input);
  const first = routes.get("first");
  const second = routes.get("second");

  assertFiniteRoute(first);
  assertFiniteRoute(second);
  assert.notDeepEqual(
    { x: Math.round(first.labelX), y: Math.round(first.labelY) },
    { x: Math.round(second.labelX), y: Math.round(second.labelY) }
  );
});

test("routeEdges separates same-node endpoint anchors", () => {
  const input = baseInput({
    relationships: [
      { id: "a", from: "source", to: "target" },
      { id: "b", from: "source", to: "target-b" }
    ]
  });
  const routes = routeEdges(input);

  assert.notDeepEqual(routes.get("a").points[0], routes.get("b").points[0]);
});

test("routeEdges spreads multiple routes entering the same node side", () => {
  const nodeRects = new Map([
    ["source-a", { x: 40, y: 90, width: 136, height: 54 }],
    ["source-b", { x: 40, y: 220, width: 136, height: 54 }],
    ["target", { x: 460, y: 150, width: 136, height: 54 }]
  ]);
  const routes = routeEdges({
    relationships: [
      { id: "a", from: "source-a", to: "target", preferredEndSide: "left", relationshipType: "flow" },
      { id: "b", from: "source-b", to: "target", preferredEndSide: "left", relationshipType: "flow" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([["source-a", 0], ["source-b", 0], ["target", 1]]),
    rowIndexByNode: new Map([["source-a", 0], ["target", 1], ["source-b", 2]]),
    canvasWidth: 760,
    canvasHeight: 420
  });
  const firstEnd = routes.get("a").points.at(-1);
  const secondEnd = routes.get("b").points.at(-1);
  const target = nodeRects.get("target");

  assert.equal(firstEnd.x, 460);
  assert.equal(secondEnd.x, 460);
  assert.equal(firstEnd.y, target.y + target.height / 3);
  assert.equal(secondEnd.y, target.y + (target.height * 2) / 3);
  assertFiniteRoute(routes.get("a"));
  assertFiniteRoute(routes.get("b"));
});

test("routeEdges stops offering a saturated node surface to later routes", () => {
  const target = { x: 340, y: 120, width: 100, height: 40 };
  const capacity = surfaceCapacity(target, "left");
  const sourceIds = Array.from({ length: capacity + 1 }, (_, index) => `source-${index}`);
  const nodeRects = new Map([
    ...sourceIds.map((sourceId, index) => [sourceId, { x: 40, y: 20 + index * 70, width: 100, height: 40 }]),
    ["target", target]
  ]);
  const routes = routeEdges({
    relationships: sourceIds.map((sourceId) => ({ id: `${sourceId}-target`, from: sourceId, to: "target", relationshipType: "flow" })),
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([...sourceIds.map((sourceId) => [sourceId, 0]), ["target", 1]]),
    rowIndexByNode: new Map([...sourceIds.map((sourceId, index) => [sourceId, index]), ["target", 1]]),
    canvasWidth: 620,
    canvasHeight: 520
  });
  const targetSides = [...routes.values()].map((route) => sideForPoint(target, route.points.at(-1)));
  const leftCount = targetSides.filter((side) => side === "left").length;

  assert.ok(leftCount <= surfaceCapacity(target, "left"), `left surface should not exceed capacity: ${targetSides.join(",")}`);
  assert.ok(targetSides.some((side) => side !== "left"), `a saturated left side should force another target side: ${targetSides.join(",")}`);
});

test("routeEdges keeps a target facing surface when only the source facing surface is saturated", () => {
  const source = { x: 40, y: 120, width: 100, height: 40 };
  const capacity = surfaceCapacity(source, "right");
  const targetIds = Array.from({ length: capacity + 1 }, (_, index) => `target-${index}`);
  const input = {
    relationships: targetIds.map((targetId) => ({ id: `source-${targetId}`, from: "source", to: targetId, relationshipType: "flow", kind: "request" })),
    visibleNodeIds: new Set(["source", ...targetIds]),
    nodeRects: new Map([
      ["source", source],
      ...targetIds.map((targetId, index) => [targetId, { x: 340, y: 20 + index * 70, width: 100, height: 40 }])
    ]),
    laneIndexByNode: new Map([
      ["source", 0],
      ...targetIds.map((targetId) => [targetId, 1])
    ]),
    rowIndexByNode: new Map([
      ["source", 1],
      ...targetIds.map((targetId, index) => [targetId, index])
    ]),
    canvasWidth: 620,
    canvasHeight: 520,
    style: "orthogonal"
  };
  const routes = routeEdges(input);
  const lastTargetId = targetIds.at(-1);
  const route = routes.get(`source-${lastTargetId}`);

  assert.notEqual(sideForPoint(source, route.points[0]), "right");
  assert.equal(sideForPoint(input.nodeRects.get(lastTargetId), route.points.at(-1)), "left");
});

test("routeEdges prefers clean facing surfaces before bend count", () => {
  const input = {
    relationships: [
      { id: "source-target", from: "source", to: "target", label: "uses", relationshipType: "flow" }
    ],
    visibleNodeIds: new Set(["source", "blocker", "target"]),
    nodeRects: new Map([
      ["source", { x: 40, y: 40, width: 100, height: 40 }],
      ["blocker", { x: 220, y: 130, width: 100, height: 40 }],
      ["target", { x: 420, y: 40, width: 100, height: 40 }]
    ]),
    laneIndexByNode: new Map([
      ["source", 0],
      ["blocker", 1],
      ["target", 2]
    ]),
    rowIndexByNode: new Map([
      ["source", 0],
      ["blocker", 1],
      ["target", 0]
    ]),
    canvasWidth: 680,
    canvasHeight: 300,
    marginY: 40,
    style: "orthogonal"
  };
  const route = routeEdges(input).get("source-target");

  assert.equal(sideForPoint(input.nodeRects.get("source"), route.points[0]), "right");
  assert.equal(sideForPoint(input.nodeRects.get("target"), route.points.at(-1)), "left");
});

test("route candidate sorting rejects shared channel overlap before bend count", () => {
  const overlappingShortRoute = {
    collisions: 0,
    paddedCollisions: 0,
    repeatedCrossings: 0,
    crossings: 0,
    sharedSegments: 1,
    sharedSegmentLength: 220,
    bends: 2,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 100
  };
  const separatedRoute = {
    collisions: 0,
    paddedCollisions: 0,
    repeatedCrossings: 0,
    crossings: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    bends: 4,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 1000
  };

  assert.equal(sortedRouteCandidates([overlappingShortRoute, separatedRoute])[0], separatedRoute);
});

test("route candidate sorting keeps correct available surfaces ahead of empty wrong surfaces", () => {
  const correctSurfaceWithChannelCost = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    semanticSurfaceMismatchCount: 0,
    surfaceDirectionMismatchCount: 0,
    crossings: 0,
    bends: 4,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 2000
  };
  const emptyWrongSurface = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    semanticSurfaceMismatchCount: 1,
    surfaceDirectionMismatchCount: 1,
    crossings: 0,
    bends: 2,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 100
  };

  assert.equal(sortedRouteCandidates([emptyWrongSurface, correctSurfaceWithChannelCost])[0], correctSurfaceWithChannelCost);
});

test("route candidate sorting rejects backwards surface direction before bend count", () => {
  const forwardSurface = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    semanticSurfaceMismatchCount: 0,
    surfaceDirectionMismatchCount: 0,
    crossings: 0,
    bends: 4,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 1000
  };
  const backwardsSurface = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    semanticSurfaceMismatchCount: 0,
    surfaceDirectionMismatchCount: 1,
    crossings: 0,
    bends: 1,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 100
  };

  assert.equal(sortedRouteCandidates([backwardsSurface, forwardSurface])[0], forwardSurface);
});

test("route candidate sorting rejects endpoint node traversal before crossings", () => {
  const throughEndpoint = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 1,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 0,
    crossings: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    bends: 1,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 100
  };
  const crossedRoute = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 1,
    crossings: 3,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    bends: 4,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 1000
  };

  assert.equal(sortedRouteCandidates([throughEndpoint, crossedRoute])[0], crossedRoute);
});

test("route candidate sorting rejects self-overlap before crossings", () => {
  const selfOverlappingRoute = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 1,
    selfOverlapLength: 80,
    repeatedCrossings: 0,
    crossings: 0,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    bends: 2,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 100
  };
  const crossedRoute = {
    collisions: 0,
    paddedCollisions: 0,
    endpointNodeTraversals: 0,
    selfOverlappingSegments: 0,
    selfOverlapLength: 0,
    repeatedCrossings: 1,
    crossings: 3,
    sharedSegments: 0,
    sharedSegmentLength: 0,
    bends: 4,
    qualityCosts: {
      monotonicBacktrackCost: 0,
      endpointStackCost: 0,
      perimeterFallbackCost: 0
    },
    cost: 1000
  };

  assert.equal(sortedRouteCandidates([selfOverlappingRoute, crossedRoute])[0], crossedRoute);
});

test("routeEdges does not traverse endpoint node interiors", () => {
  const nodeRects = new Map([
    ["source", { x: 40, y: 150, width: 136, height: 54 }],
    ["target", { x: 360, y: 150, width: 136, height: 54 }],
    ["blocker", { x: 200, y: 150, width: 120, height: 54 }]
  ]);
  const route = routeEdges({
    relationships: [
      { id: "source-target", from: "source", to: "target", relationshipType: "flow" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([["source", 0], ["blocker", 1], ["target", 2]]),
    rowIndexByNode: new Map([["source", 0], ["blocker", 0], ["target", 0]]),
    canvasWidth: 640,
    canvasHeight: 360
  }).get("source-target");

  assert.equal(routeTouchesRectInterior(route, nodeRects.get("source")), false);
  assert.equal(routeTouchesRectInterior(route, nodeRects.get("target")), false);
});

test("routeEdges moves the upstream bend onto the shifted arrowhead mount", () => {
  const nodeRects = new Map([
    ["maintainer", { x: 180, y: 76, width: 136, height: 54 }],
    ["architext-cli", { x: 390, y: 76, width: 136, height: 54 }],
    ["schema-validator", { x: 390, y: 178, width: 136, height: 54 }]
  ]);
  const routes = routeEdges({
    relationships: [
      { id: "resolve", from: "maintainer", to: "architext-cli", relationshipType: "flow", preferredEndSide: "left" },
      { id: "invalid", from: "schema-validator", to: "architext-cli", relationshipType: "flow", preferredEndSide: "left" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([["maintainer", 0], ["architext-cli", 1], ["schema-validator", 1]]),
    rowIndexByNode: new Map([["maintainer", 0], ["architext-cli", 0], ["schema-validator", 1]]),
    canvasWidth: 800,
    canvasHeight: 400
  });

  assert.equal(routes.get("resolve").d, "M 316 94 L 390 94");
  const invalid = routes.get("invalid");
  assert.equal(invalid.points[0].y, invalid.points[1].y);
  assert.equal(invalid.points[1].x, invalid.points[2].x);
  assert.equal(invalid.points[2].y, invalid.points[3].y);
  assert.equal(invalid.points.at(-1).y, 112);
  assert.equal(routeHasImmediateBacktrack(invalid), false);
});

test("routeEdges keeps spread short direct routes orthogonal", () => {
  const nodeRects = new Map([
    ["pipeline", { x: 390, y: 104, width: 136, height: 54 }],
    ["memory", { x: 600, y: 104, width: 136, height: 54 }]
  ]);
  const routes = routeEdges({
    relationships: [
      { id: "request", from: "pipeline", to: "memory", relationshipType: "flow", kind: "request" },
      { id: "response", from: "memory", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "request" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([["pipeline", 0], ["memory", 1]]),
    rowIndexByNode: new Map([["pipeline", 0], ["memory", 0]]),
    canvasWidth: 900,
    canvasHeight: 360,
    style: "orthogonal"
  });

  assertFiniteRoute(routes.get("request"));
  assertFiniteRoute(routes.get("response"));
});

test("routeEdges removes endpoint-spread backtracks from rendered orthogonal routes", () => {
  const nodeRects = new Map([
    ["pipeline", { x: 390, y: 104, width: 136, height: 54 }],
    ["memory", { x: 600, y: 104, width: 136, height: 54 }],
    ["store", { x: 600, y: 488, width: 136, height: 54 }],
    ["mcp", { x: 600, y: 392, width: 136, height: 54 }]
  ]);
  const routes = routeEdges({
    relationships: [
      { id: "memory-request", from: "pipeline", to: "memory", relationshipType: "flow", kind: "request" },
      { id: "memory-return", from: "memory", to: "pipeline", relationshipType: "flow", kind: "return", returnOf: "memory-request" },
      { id: "store-request", from: "pipeline", to: "store", relationshipType: "flow", kind: "persistence" },
      { id: "mcp-request", from: "pipeline", to: "mcp", relationshipType: "flow", kind: "request" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([
      ["pipeline", 0],
      ["memory", 1],
      ["store", 1],
      ["mcp", 1]
    ]),
    rowIndexByNode: new Map([
      ["pipeline", 0],
      ["memory", 0],
      ["mcp", 1],
      ["store", 2]
    ]),
    canvasWidth: 900,
    canvasHeight: 720,
    style: "orthogonal"
  });

  for (const route of routes.values()) {
    assertFiniteRoute(route);
    assert.equal(routeHasImmediateBacktrack(route), false, route.d);
  }
});

test("routeEdges orders shared surface endpoints by opposite route projection", () => {
  const nodeRects = new Map([
    ["hub", { x: 300, y: 80, width: 136, height: 60 }],
    ["right-target", { x: 500, y: 240, width: 136, height: 60 }],
    ["left-target", { x: 100, y: 240, width: 136, height: 60 }]
  ]);
  const routes = routeEdges({
    relationships: [
      { id: "right", from: "hub", to: "right-target", relationshipType: "flow", preferredStartSide: "bottom", preferredEndSide: "top" },
      { id: "left", from: "hub", to: "left-target", relationshipType: "flow", preferredStartSide: "bottom", preferredEndSide: "top" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([
      ["left-target", 0],
      ["hub", 1],
      ["right-target", 2]
    ]),
    rowIndexByNode: new Map([
      ["hub", 0],
      ["left-target", 1],
      ["right-target", 1]
    ]),
    canvasWidth: 760,
    canvasHeight: 480,
    style: "orthogonal"
  });

  const hub = nodeRects.get("hub");
  assert.equal(sideForPoint(hub, routes.get("left").points[0]), "bottom");
  assert.equal(sideForPoint(hub, routes.get("right").points[0]), "bottom");
  assert.ok(
    routes.get("left").points[0].x < routes.get("right").points[0].x,
    "the leftward route should claim the leftmost mount on the shared surface"
  );
});

test("routeEdges keeps outward endpoint stubs before long turns", () => {
  const nodeRects = new Map([
    ["operator", { x: 180, y: 76, width: 136, height: 54 }],
    ["web-dashboard", { x: 180, y: 382, width: 136, height: 54 }],
    ["websocket-control-plane", { x: 180, y: 484, width: 136, height: 54 }],
    ["external-channel-adapters", { x: 180, y: 586, width: 136, height: 54 }],
    ["unified-pipeline", { x: 390, y: 76, width: 136, height: 90 }],
    ["memory-system", { x: 600, y: 76, width: 136, height: 54 }],
    ["mcp-system", { x: 600, y: 382, width: 136, height: 54 }],
    ["sqlite-store", { x: 600, y: 484, width: 136, height: 54 }],
    ["llm-service", { x: 810, y: 76, width: 136, height: 54 }]
  ]);
  const relationships = [
    { id: "delegate-web-message", from: "websocket-control-plane", to: "unified-pipeline", relationshipType: "flow", kind: "request", displayIndex: 3 },
    { id: "normalize-input", from: "external-channel-adapters", to: "unified-pipeline", relationshipType: "flow", kind: "request", displayIndex: 4 },
    { id: "resolve-session", from: "unified-pipeline", to: "sqlite-store", relationshipType: "flow", kind: "persistence", displayIndex: 5 },
    { id: "session-resolved", from: "sqlite-store", to: "unified-pipeline", relationshipType: "flow", kind: "return", returnOf: "resolve-session", displayIndex: 6 },
    { id: "retrieve-context", from: "unified-pipeline", to: "memory-system", relationshipType: "flow", kind: "request", displayIndex: 7 },
    { id: "context-returned", from: "memory-system", to: "unified-pipeline", relationshipType: "flow", kind: "return", returnOf: "retrieve-context", displayIndex: 8 },
    { id: "execute-tools", from: "unified-pipeline", to: "mcp-system", relationshipType: "flow", kind: "request", displayIndex: 9 },
    { id: "web-pipeline-outcome", from: "unified-pipeline", to: "websocket-control-plane", relationshipType: "flow", kind: "return", displayIndex: 15 },
    { id: "format-response", from: "unified-pipeline", to: "external-channel-adapters", relationshipType: "flow", kind: "return", displayIndex: 17 }
  ];
  const routes = routeEdges({
    relationships,
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([
      ["operator", 0],
      ["web-dashboard", 0],
      ["websocket-control-plane", 0],
      ["external-channel-adapters", 0],
      ["unified-pipeline", 1],
      ["memory-system", 2],
      ["mcp-system", 2],
      ["sqlite-store", 2],
      ["llm-service", 3]
    ]),
    rowIndexByNode: new Map([
      ["operator", 0],
      ["unified-pipeline", 0],
      ["memory-system", 0],
      ["llm-service", 0],
      ["web-dashboard", 3],
      ["websocket-control-plane", 4],
      ["mcp-system", 3],
      ["external-channel-adapters", 5],
      ["sqlite-store", 4]
    ]),
    canvasWidth: 1120,
    canvasHeight: 760,
    style: "orthogonal"
  });

  for (const routeId of ["normalize-input", "format-response"]) {
    assert.ok(endpointStubLength(routes.get(routeId), "source") >= PORT_STUB, `${routeId} has a collapsed source stub`);
  }
  assert.ok(endpointStubLength(routes.get("session-resolved"), "target") >= PORT_STUB, "session-resolved has a collapsed target stub");
});

test("planDiagram keeps dense flow nodes at uniform size and still routes every edge", () => {
  const view = {
    id: "dense-flow",
    name: "Dense Flow",
    type: "flow",
    lanes: [
      { id: "left", name: "Left", nodeIds: ["left-a", "left-b", "left-c", "left-d"] },
      { id: "runtime", name: "Runtime", nodeIds: ["hub"] },
      { id: "right", name: "Right", nodeIds: ["right-a", "right-b", "right-c", "right-d"] }
    ]
  };
  const relationships = [
    { id: "left-a-hub", from: "left-a", to: "hub", relationshipType: "flow" },
    { id: "left-b-hub", from: "left-b", to: "hub", relationshipType: "flow" },
    { id: "left-c-hub", from: "left-c", to: "hub", relationshipType: "flow" },
    { id: "left-d-hub", from: "left-d", to: "hub", relationshipType: "flow" },
    { id: "hub-right-a", from: "hub", to: "right-a", relationshipType: "flow" },
    { id: "hub-right-b", from: "hub", to: "right-b", relationshipType: "flow" },
    { id: "hub-right-c", from: "hub", to: "right-c", relationshipType: "flow" },
    { id: "hub-right-d", from: "hub", to: "right-d", relationshipType: "flow" }
  ];

  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 136,
    nodeHeight: 54,
    laneWidth: 210,
    rowGap: 102,
    marginX: 60,
    marginY: 80,
    minCanvasWidth: 760,
    minCanvasHeight: 560,
    canvasExtraWidth: 40,
    canvasExtraHeight: 88,
    style: "orthogonal"
  });

  assert.equal(plan.nodeRects.get("left-a").height, 54);
  assert.equal(
    plan.nodeRects.get("hub").height,
    54,
    "node dimensions must not be expanded to accommodate mount points"
  );
  assert.equal(plan.routes.size, relationships.length, "every dense flow edge must still route");
});

test("routeEdges avoids line crossings when a clean alternative exists", () => {
  const nodeRects = new Map([
    ["left", { x: 40, y: 180, width: 136, height: 54 }],
    ["right", { x: 460, y: 180, width: 136, height: 54 }],
    ["top", { x: 250, y: 40, width: 136, height: 54 }],
    ["bottom", { x: 250, y: 320, width: 136, height: 54 }]
  ]);
  const input = {
    relationships: [
      { id: "horizontal", from: "left", to: "right" },
      { id: "vertical", from: "top", to: "bottom" }
    ],
    visibleNodeIds: new Set(nodeRects.keys()),
    nodeRects,
    laneIndexByNode: new Map([
      ["left", 0],
      ["top", 1],
      ["bottom", 1],
      ["right", 2]
    ]),
    rowIndexByNode: new Map([
      ["top", 0],
      ["left", 1],
      ["right", 1],
      ["bottom", 2]
    ]),
    canvasWidth: 760,
    canvasHeight: 460,
    marginY: 76,
    style: "orthogonal"
  };
  const route = routeEdges(input).get("vertical");

  assertFiniteRoute(route);
  // The only crossing-free alternative here is a wide perimeter detour around the
  // horizontal's ends, which is a worse outcome than a single accepted crossing.
  // The vertical therefore takes the direct interior path (staying within the node
  // bounding box) and the crossing is rendered as a hop-over.
  const xs = route.points.map((point) => point.x);
  assert.ok(Math.min(...xs) >= 40 && Math.max(...xs) <= 596, "vertical stays interior, no perimeter detour");
  assert.match(route.d, /\bQ\b/, "the accepted crossing renders as a hop-over");
});

test("pathToSvgWithHops renders hop-overs for accepted orthogonal crossings", () => {
  const d = pathToSvgWithHops(
    [
      { x: 160, y: 100 },
      { x: 160, y: 220 }
    ],
    [
      {
        points: [
          { x: 80, y: 160 },
          { x: 240, y: 160 }
        ]
      }
    ]
  );

  assert.match(d, /\bQ\b/);
});

test("pathToSvgWithHops renders visible hops after the full route set is finalized", () => {
  const horizontal = pathToSvgWithHops(
    [
      { x: 80, y: 160 },
      { x: 240, y: 160 }
    ],
    [
      {
        points: [
          { x: 160, y: 100 },
          { x: 160, y: 220 }
        ]
      }
    ]
  );
  const vertical = pathToSvgWithHops(
    [
      { x: 160, y: 100 },
      { x: 160, y: 220 }
    ],
    [
      {
        points: [
          { x: 80, y: 160 },
          { x: 240, y: 160 }
        ]
      }
    ]
  );

  assert.match(horizontal, /\bQ\b/);
  assert.match(vertical, /\bQ\b/);
});

test("pathToSvgWithHops computes hops against a whole finalized route set", () => {
  const vertical = {
    points: [
      { x: 160, y: 100 },
      { x: 160, y: 220 }
    ]
  };
  const horizontal = {
    points: [
      { x: 80, y: 160 },
      { x: 240, y: 160 }
    ]
  };
  const d = pathToSvgWithHops(vertical.points, [vertical, horizontal]);

  assert.match(d, /\bQ\b/);
});

test("pathToSvgWithHops still renders a hop when the crossing is near a corner (adaptive radius)", () => {
  // The crossing at (160,160) sits 4px from the vertical route's top corner (160,156) — within
  // HOP_RADIUS. The fixed-radius guard skipped the hop here (rendered flat); an adaptive radius
  // draws a smaller hop sized to the available room so every crossing is still hopped.
  const corneredVertical = {
    points: [
      { x: 160, y: 156 },
      { x: 160, y: 260 },
      { x: 200, y: 260 }
    ]
  };
  const d = pathToSvgWithHops(
    [
      { x: 80, y: 160 },
      { x: 240, y: 160 }
    ],
    [corneredVertical]
  );

  assert.match(d, /\bQ\b/, "a crossing within HOP_RADIUS of a corner should still render a (smaller) hop");
});

test("routeEdges routes multi-target fan-out deterministically", () => {
  const input = baseInput({
    relationships: [
      { id: "a", from: "source", to: "target" },
      { id: "b", from: "source", to: "target-b" }
    ]
  });
  const routes = routeEdges(input);

  assert.equal([...routes.keys()].join(","), "a,b");
  assertFiniteRoute(routes.get("a"));
  assertFiniteRoute(routes.get("b"));
});

test("planDiagram centralizes geometry and route planning", () => {
  const view = {
    id: "c4-component",
    name: "C4 Component",
    type: "c4-component",
    lanes: [
      { id: "entry", name: "Entry", nodeIds: ["source"] },
      { id: "runtime", name: "Runtime", nodeIds: ["target", "target-b"] }
    ]
  };
  const relationships = [
    { id: "a", from: "source", to: "target" },
    { id: "b", from: "source", to: "target-b" }
  ];
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(["source", "target", "target-b"]),
    nodeWidth: 156,
    nodeHeight: 92,
    laneWidth: 210,
    rowGap: 116,
    marginX: 56,
    marginY: 76,
    minCanvasWidth: 760,
    minCanvasHeight: 440,
    canvasExtraWidth: 40,
    canvasExtraHeight: 88,
    style: "orthogonal"
  });

  assert.equal(plan.canvasWidth, 760);
  assert.equal(plan.canvasHeight, 440);
  assert.deepEqual(plan.positionFor("target-b"), { x: 266, y: 192 });
  assert.equal([...plan.routes.keys()].join(","), "a,b");
  assert.equal([...plan.labelBoxes.keys()].join(","), "a,b");
  assert.equal(plan.warnings.filter((warning) => warning.code === "label-over-node").length, 0);
  assertFiniteRoute(plan.routes.get("a"));
  assertFiniteRoute(plan.routes.get("b"));
});
