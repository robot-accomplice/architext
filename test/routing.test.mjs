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

test("routeEdges produces deterministic finite routes", () => {
  const input = baseInput();
  const first = routeEdges(input).get("source-target");
  const second = routeEdges(input).get("source-target");

  assertFiniteRoute(first);
  assert.deepEqual(second, first);
});

test("routeEdges renders a whole route set with the selected visual style", () => {
  const orthogonal = routeEdges(baseInput({ style: "orthogonal" })).get("source-target");
  const curved = routeEdges(baseInput({ style: "curved" })).get("source-target");

  assertFiniteRoute(orthogonal, "orthogonal");
  assertFiniteRoute(curved, "curved");
  assert.doesNotMatch(orthogonal.d, /\b[QC]\b/);
  assert.match(curved.d, /\bQ\b/);
  assert.notDeepEqual(curved.samples, orthogonal.samples);
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

test("routeEdges avoids non-endpoint node bodies when a blocker is between endpoints", () => {
  const input = baseInput();
  const route = routeEdges(input).get("source-target");

  assertFiniteRoute(route);
  assert.equal(routeIntersectsRect(route, input.nodeRects.get("blocker"), 0), false);
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
