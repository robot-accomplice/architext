import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { planDiagram } from "../docs/architext/src/routing/planDiagram.js";
import { relationshipLabel } from "../docs/architext/src/routing/relationshipLabels.js";
import { pathToSvgWithHops, routeIntersectsRect } from "../docs/architext/src/routing/routeEdges.js";

const architextNodes = JSON.parse(readFileSync(new URL("../docs/architext/data/nodes.json", import.meta.url), "utf8")).nodes;
const architextFlows = JSON.parse(readFileSync(new URL("../docs/architext/data/flows.json", import.meta.url), "utf8")).flows;
const architextViews = JSON.parse(readFileSync(new URL("../docs/architext/data/views.json", import.meta.url), "utf8")).views;
const architextNodesById = new Map(architextNodes.map((node) => [node.id, node]));

function planFixture(view, relationships, overrides = {}) {
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  return planDiagram({
    view,
    relationships,
    visibleNodeIds,
    nodeWidth: overrides.nodeWidth ?? 144,
    nodeHeight: overrides.nodeHeight ?? 62,
    laneWidth: overrides.laneWidth ?? 218,
    rowGap: overrides.rowGap ?? 116,
    marginX: overrides.marginX ?? 72,
    marginY: overrides.marginY ?? 76,
    minCanvasWidth: overrides.minCanvasWidth ?? 820,
    minCanvasHeight: overrides.minCanvasHeight ?? 520,
    canvasExtraWidth: overrides.canvasExtraWidth ?? 80,
    canvasExtraHeight: overrides.canvasExtraHeight ?? 96,
    style: overrides.style ?? "orthogonal"
  });
}

function assertPlanFitness(plan, relationships, options = {}) {
  assert.equal(plan.routes.size, relationships.length);
  assert.equal(plan.labelBoxes.size, relationships.length);
  for (const relationship of relationships) {
    const route = plan.routes.get(relationship.id);
    assert.ok(route, `missing route for ${relationship.id}`);
    assert.equal(Number.isFinite(route.labelX), true);
    assert.equal(Number.isFinite(route.labelY), true);
    assert.ok(route.samples.length > 0);
    assert.ok(route.bends <= (options.maxBends ?? 8), `${relationship.id} has too many bends: ${route.bends}`);
    assertRouteQualityCosts(route);
    assertPerpendicularContact(route, plan.nodeRects.get(relationship.from), plan.nodeRects.get(relationship.to), relationship.id);
    assertNoNodeBodyCollisions(route, plan, relationship);
    assertLabelAvoidsNodes(plan.labelBoxes.get(relationship.id), plan, relationship);
  }
  assertNoRepeatedCrossings([...plan.routes.values()]);
}

function assertRouteQualityCosts(route) {
  for (const key of ["lengthCost", "bendCost", "doglegCost", "labelMovementCost", "labelNodeConflictCost", "labelConflictCost"]) {
    assert.equal(Number.isFinite(route.qualityCosts[key]), true, `${key} must be finite`);
  }
  const total = Object.values(route.qualityCosts).reduce((sum, value) => sum + value, 0);
  assert.equal(Math.round(route.cost * 1000) / 1000, Math.round(total * 1000) / 1000);
}

function assertNoNodeBodyCollisions(route, plan, relationship) {
  for (const [nodeId, rect] of plan.nodeRects) {
    if (nodeId === relationship.from || nodeId === relationship.to) continue;
    assert.equal(routeIntersectsRect(route, rect, 0), false, `${relationship.id} intersects ${nodeId}`);
  }
}

function assertLabelAvoidsNodes(labelBox, plan, relationship) {
  assert.ok(labelBox, `missing label box for ${relationship.id}`);
  for (const [nodeId, rect] of plan.nodeRects) {
    assert.equal(rectsOverlap(labelBox, rect, 0), false, `${relationship.id} label overlaps ${nodeId}`);
  }
}

function rectsOverlap(a, b, padding = 0) {
  return (
    a.x < b.x + b.width + padding &&
    a.x + a.width > b.x - padding &&
    a.y < b.y + b.height + padding &&
    a.y + a.height > b.y - padding
  );
}

function assertPerpendicularContact(route, sourceRect, targetRect, routeId) {
  const first = route.points[0];
  const second = route.points[1];
  const beforeLast = route.points[route.points.length - 2];
  const last = route.points[route.points.length - 1];

  assert.equal(segmentLeavesRectPerpendicularly(first, second, sourceRect), true, `${routeId} leaves source non-perpendicularly`);
  assert.equal(segmentLeavesRectPerpendicularly(last, beforeLast, targetRect), true, `${routeId} enters target non-perpendicularly`);
}

function segmentLeavesRectPerpendicularly(anchor, next, rect) {
  if (anchor.x === rect.x) return next.y === anchor.y && next.x <= anchor.x;
  if (anchor.x === rect.x + rect.width) return next.y === anchor.y && next.x >= anchor.x;
  if (anchor.y === rect.y) return next.x === anchor.x && next.y <= anchor.y;
  if (anchor.y === rect.y + rect.height) return next.x === anchor.x && next.y >= anchor.y;
  return false;
}

function orthogonalSegments(route) {
  const segments = [];
  for (let index = 0; index < route.points.length - 1; index += 1) {
    const start = route.points[index];
    const end = route.points[index + 1];
    if (start.x === end.x || start.y === end.y) segments.push({ start, end });
  }
  return segments;
}

function crossingPoint(a, b) {
  if (a.start.y === a.end.y && b.start.x === b.end.x) {
    return horizontalVerticalCrossing(a, b);
  }
  if (a.start.x === a.end.x && b.start.y === b.end.y) {
    return horizontalVerticalCrossing(b, a);
  }
  return null;
}

function horizontalVerticalCrossing(horizontal, vertical) {
  const minX = Math.min(horizontal.start.x, horizontal.end.x);
  const maxX = Math.max(horizontal.start.x, horizontal.end.x);
  const minY = Math.min(vertical.start.y, vertical.end.y);
  const maxY = Math.max(vertical.start.y, vertical.end.y);
  const x = vertical.start.x;
  const y = horizontal.start.y;
  if (x <= minX || x >= maxX || y <= minY || y >= maxY) return null;
  return `${x},${y}`;
}

function assertNoRepeatedCrossings(routes) {
  for (let leftIndex = 0; leftIndex < routes.length; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < routes.length; rightIndex += 1) {
      const crossings = new Set();
      for (const left of orthogonalSegments(routes[leftIndex])) {
        for (const right of orthogonalSegments(routes[rightIndex])) {
          const crossing = crossingPoint(left, right);
          if (crossing) crossings.add(crossing);
        }
      }
      assert.ok(crossings.size <= 1, `routes ${leftIndex} and ${rightIndex} cross ${crossings.size} times`);
    }
  }
}

function planMetrics(plan) {
  const routes = [...plan.routes.values()];
  return {
    routes: routes.length,
    warnings: plan.warnings.length,
    bends: routes.reduce((sum, route) => sum + route.bends, 0),
    crossings: routes.reduce((sum, route) => sum + (route.crossings ?? 0), 0),
    repeatedCrossings: routes.reduce((sum, route) => sum + (route.repeatedCrossings ?? 0), 0),
    doglegCost: Math.round(routes.reduce((sum, route) => sum + route.qualityCosts.doglegCost, 0)),
    monotonicBacktrackCost: Math.round(routes.reduce((sum, route) => sum + route.qualityCosts.monotonicBacktrackCost, 0)),
    endpointStackCost: Math.round(routes.reduce((sum, route) => sum + route.qualityCosts.endpointStackCost, 0)),
    labelMovementCost: Math.round(routes.reduce((sum, route) => sum + route.qualityCosts.labelMovementCost, 0)),
    labelConflictCost: Math.round(routes.reduce((sum, route) => sum + route.qualityCosts.labelConflictCost, 0)),
    labelNodeConflictCost: Math.round(routes.reduce((sum, route) => sum + route.qualityCosts.labelNodeConflictCost, 0)),
    perimeterFallbackRoutes: routes.filter((route) => route.qualityCosts.perimeterFallbackCost > 0).length,
    perimeterFallbackCost: Math.round(routes.reduce((sum, route) => sum + route.qualityCosts.perimeterFallbackCost, 0))
  };
}

function assertMetricBudget(name, metrics, budget) {
  for (const [key, max] of Object.entries(budget)) {
    assert.ok(metrics[key] <= max, `${name} ${key}=${metrics[key]} exceeds ${max}; metrics=${JSON.stringify(metrics)}`);
  }
}

function structuralRelationshipsForView(view) {
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  return [...visibleNodeIds].flatMap((nodeId) => {
    const node = architextNodesById.get(nodeId);
    return (node?.dependencies ?? [])
      .filter((dependencyId) => visibleNodeIds.has(dependencyId))
      .map((dependencyId) => ({
        id: `${nodeId}->${dependencyId}`,
        from: nodeId,
        to: dependencyId,
        label: relationshipLabel(node, architextNodesById.get(dependencyId))
      }));
  });
}

function axisAlignedSegments(route) {
  const segments = [];
  for (let index = 0; index < route.points.length - 1; index += 1) {
    const start = route.points[index];
    const end = route.points[index + 1];
    if (start.x === end.x) {
      segments.push({
        orientation: "vertical",
        x: start.x,
        min: Math.min(start.y, end.y),
        max: Math.max(start.y, end.y)
      });
    } else if (start.y === end.y) {
      segments.push({
        orientation: "horizontal",
        y: start.y,
        min: Math.min(start.x, end.x),
        max: Math.max(start.x, end.x)
      });
    }
  }
  return segments;
}

function sharedSegmentLength(left, right) {
  if (left.orientation !== right.orientation) return 0;
  if (left.orientation === "horizontal" && left.y !== right.y) return 0;
  if (left.orientation === "vertical" && left.x !== right.x) return 0;
  return Math.max(0, Math.min(left.max, right.max) - Math.max(left.min, right.min));
}

function sharedOrthogonalSegmentCount(plan) {
  const routes = [...plan.routes.entries()];
  let sharedSegments = 0;
  for (let leftIndex = 0; leftIndex < routes.length; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < routes.length; rightIndex += 1) {
      for (const leftSegment of axisAlignedSegments(routes[leftIndex][1])) {
        for (const rightSegment of axisAlignedSegments(routes[rightIndex][1])) {
          if (sharedSegmentLength(leftSegment, rightSegment) > 1) {
            sharedSegments += 1;
          }
        }
      }
    }
  }
  return sharedSegments;
}

function flowRelationshipsForView(flow, view) {
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  return flow.steps
    .map((step, index) => ({
      id: step.id,
      from: step.from,
      to: step.to,
      label: `${index + 1}. ${step.action}`,
      relationshipType: "flow",
      stepId: step.id,
      displayIndex: index + 1
    }))
    .filter((relationship) => visibleNodeIds.has(relationship.from) && visibleNodeIds.has(relationship.to));
}

test("fitness: complex fan-out keeps routes readable around blockers", () => {
  const view = {
    id: "complex-fan-out",
    name: "Complex Fan-Out",
    type: "system-map",
    lanes: [
      { id: "source", name: "Source", nodeIds: ["source"] },
      { id: "middle", name: "Middle", nodeIds: ["blocker-a", "blocker-b", "blocker-c"] },
      { id: "targets", name: "Targets", nodeIds: ["target-a", "target-b", "target-c", "target-d"] }
    ]
  };
  const relationships = ["target-a", "target-b", "target-c", "target-d"].map((targetId) => ({
    id: `source-${targetId}`,
    from: "source",
    to: targetId,
    label: `routes to ${targetId}`
  }));
  const plan = planFixture(view, relationships);

  assertPlanFitness(plan, relationships, { maxBends: 8 });
  assertMetricBudget("complex-fan-out", planMetrics(plan), {
    warnings: 0,
    bends: 10,
    repeatedCrossings: 0,
    doglegCost: 0,
    monotonicBacktrackCost: 0,
    endpointStackCost: 0,
    labelConflictCost: 0,
    labelNodeConflictCost: 0,
    perimeterFallbackRoutes: 0
  });
});

test("fitness: complex fan-in keeps endpoint stacks distinguishable", () => {
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
    label: `${sourceId} feeds target`
  }));
  const plan = planFixture(view, relationships);

  assertPlanFitness(plan, relationships, { maxBends: 8 });
  assert.equal(new Set(relationships.map((relationship) => JSON.stringify(plan.routes.get(relationship.id).points.at(-1)))).size, relationships.length);
  assertMetricBudget("complex-fan-in", planMetrics(plan), {
    warnings: 0,
    bends: 10,
    repeatedCrossings: 0,
    doglegCost: 0,
    monotonicBacktrackCost: 0,
    endpointStackCost: 0,
    labelConflictCost: 0,
    labelNodeConflictCost: 0,
    perimeterFallbackRoutes: 0
  });
});

test("fitness: C4-style component lanes use the shared planner", () => {
  const view = {
    id: "complex-c4-component",
    name: "Complex C4 Component",
    type: "c4-component",
    lanes: [
      { id: "entry", name: "Entry", nodeIds: ["cli"] },
      { id: "runtime", name: "Runtime", nodeIds: ["server", "validator", "builder"] },
      { id: "data", name: "Data", nodeIds: ["schema", "project-data", "static-output"] }
    ]
  };
  const relationships = [
    { id: "cli-server", from: "cli", to: "server", label: "serves" },
    { id: "cli-validator", from: "cli", to: "validator", label: "validates" },
    { id: "server-project-data", from: "server", to: "project-data", label: "reads" },
    { id: "validator-schema", from: "validator", to: "schema", label: "uses" },
    { id: "builder-static-output", from: "builder", to: "static-output", label: "writes" }
  ];
  const plan = planFixture(view, relationships, {
    nodeWidth: 156,
    nodeHeight: 92,
    laneWidth: 210,
    rowGap: 116,
    minCanvasWidth: 760,
    minCanvasHeight: 440
  });

  assertPlanFitness(plan, relationships, { maxBends: 8 });
  assertMetricBudget("complex-c4-component", planMetrics(plan), {
    warnings: 0,
    bends: 24,
    repeatedCrossings: 0,
    doglegCost: 0,
    monotonicBacktrackCost: 0,
    endpointStackCost: 0,
    labelConflictCost: 0,
    labelNodeConflictCost: 0,
    perimeterFallbackRoutes: 0
  });
});

test("fitness: Architext deployment avoids shared structural route segments", () => {
  const view = architextViews.find((candidate) => candidate.id === "deployment");
  assert.ok(view, "missing Architext deployment view");
  const relationships = structuralRelationshipsForView(view);
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

  assertPlanFitness(plan, relationships, { maxBends: 10 });
  assert.equal(sharedOrthogonalSegmentCount(plan), 0, "deployment routes share visible segments");
});

test("fitness: Architext Data/Risks active flows avoid shared route segments", () => {
  for (const viewId of ["dataflow", "risk-overlay"]) {
    const view = architextViews.find((candidate) => candidate.id === viewId);
    assert.ok(view, `missing Architext ${viewId} view`);
    for (const flow of architextFlows) {
      const relationships = flowRelationshipsForView(flow, view);
      if (relationships.length < 2) continue;
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
      minCanvasWidth: 0,
      minCanvasHeight: 340,
      canvasExtraWidth: 132,
      canvasExtraHeight: 88,
      style: "orthogonal"
    });

      assertPlanFitness(plan, relationships, { maxBends: 10 });
      assert.equal(sharedOrthogonalSegmentCount(plan), 0, `${viewId}:${flow.id} routes share visible segments`);
    }
  }
});

test("fitness: complex crossing hops render every accepted perpendicular crossing", () => {
  const d = pathToSvgWithHops(
    [
      { x: 180, y: 80 },
      { x: 180, y: 300 }
    ],
    [
      {
        points: [
          { x: 80, y: 140 },
          { x: 320, y: 140 }
        ]
      },
      {
        points: [
          { x: 80, y: 240 },
          { x: 320, y: 240 }
        ]
      }
    ]
  );

  assert.equal((d.match(/\bQ\b/g) ?? []).length, 2);
});

test("fitness: complex too-close layout reports an explicit routing warning", () => {
  const view = {
    id: "complex-too-close",
    name: "Complex Too Close",
    type: "system-map",
    lanes: [
      { id: "left", name: "Left", nodeIds: ["source"] },
      { id: "right", name: "Right", nodeIds: ["target"] }
    ]
  };
  const relationships = [
    { id: "source-target", from: "source", to: "target", label: "too close" }
  ];
  const plan = planFixture(view, relationships, {
    laneWidth: 150,
    minCanvasWidth: 420,
    minCanvasHeight: 260
  });
  const route = plan.routes.get("source-target");

  assert.ok(route, "missing cramped route");
  assert.ok(route.warnings.some((warning) => warning.code === "nodes-too-close"));
  assert.ok(plan.warnings.some((warning) => warning.code === "nodes-too-close"));
  assertRouteQualityCosts(route);
  assertMetricBudget("complex-too-close", planMetrics(plan), {
    warnings: 1,
    repeatedCrossings: 0,
    labelConflictCost: 0,
    labelNodeConflictCost: 0
  });
});
