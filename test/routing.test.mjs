import assert from "node:assert/strict";
import test from "node:test";
import { planDiagram } from "../docs/architext/src/routing/planDiagram.js";
import { pathToSvgWithHops, routeEdges, routeIntersectsRect } from "../docs/architext/src/routing/routeEdges.js";

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
        `orthogonal route contains diagonal segment ${JSON.stringify({ start, end, route: route.d })}`
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

function rectsOverlap(a, b, padding = 0) {
  return (
    a.x < b.x + b.width + padding &&
    a.x + a.width > b.x - padding &&
    a.y < b.y + b.height + padding &&
    a.y + a.height > b.y - padding
  );
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
  assert.equal(routes.get("invalid").d, "M 458 178 L 458 160 L 458 112 L 372 112 L 390 112");
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
  assert.doesNotMatch(route.d, /\bQ\b/);
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

test("pathToSvgWithHops keeps east-west segments straight", () => {
  const d = pathToSvgWithHops(
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

  assert.doesNotMatch(d, /\bQ\b/);
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
