import assert from "node:assert/strict";
import test from "node:test";
import { semanticSurfaceOptions } from "../viewer/src/routing/routeIntent.js";

// Two nodes on the same row with an intermediary node blocking the facing
// (horizontal) corridor between them. Per the obstacle-aware surface rule, both
// endpoints must be free to escape to a parallel (perpendicular) surface so the
// edge routes over/under the blocker instead of exiting the blocked facing side.
test("blocked horizontal corridor offers a parallel escape on BOTH endpoints", () => {
  const fromRect = { x: 0, y: 100, width: 100, height: 50 };
  const toRect = { x: 400, y: 100, width: 100, height: 50 };
  const blocker = { x: 200, y: 100, width: 100, height: 50 };

  const options = semanticSurfaceOptions({
    expectedSides: { source: "right", target: "left" },
    relationship: { id: "r", relationshipType: "flow", flowId: "f", stepId: "s" },
    fromRect,
    toRect,
    blockerRects: [blocker],
    canvasWidth: 600,
    canvasHeight: 300
  });

  assert.ok(options.source.has("right"), "source keeps its facing surface as an option");
  assert.ok(options.target.has("left"), "target keeps its facing surface as an option");
  assert.equal(options.source.size > 1, true, "source must also offer a parallel escape surface");
  assert.equal(options.target.size > 1, true, "target must also offer a parallel escape surface");
});

// A blocked corridor between endpoints that are NOT coplanar should keep the
// original asymmetric escape (forward flows escape only the arriving target),
// so unrelated return/cross-row flows are not perturbed.
test("blocked corridor for non-coplanar endpoints escapes only the target (forward)", () => {
  const fromRect = { x: 0, y: 100, width: 100, height: 50 };
  const toRect = { x: 400, y: 260, width: 100, height: 50 };
  const blocker = { x: 200, y: 160, width: 100, height: 90 };

  const options = semanticSurfaceOptions({
    expectedSides: { source: "right", target: "left" },
    relationship: { id: "r", relationshipType: "flow", flowId: "f", stepId: "s" },
    fromRect,
    toRect,
    blockerRects: [blocker],
    canvasWidth: 600,
    canvasHeight: 400
  });

  assert.equal(options.source.size, 1, "non-coplanar source keeps only its facing surface");
});

// Two nodes in the same COLUMN with intermediaries between them (vertical intent)
// must be able to escape to the near outer gutter and run straight up/down it,
// rather than mounting top/bottom and jogging into the gutter. Leftmost-column
// nodes escape LEFT (toward the adjacent free gutter), not inward.
test("vertical corridor blocked by two intermediaries escapes to the near outer gutter on both ends", () => {
  const fromRect = { x: 200, y: 0, width: 100, height: 50 };
  const toRect = { x: 200, y: 600, width: 100, height: 50 };
  const blockerOne = { x: 200, y: 180, width: 100, height: 50 };
  const blockerTwo = { x: 200, y: 360, width: 100, height: 50 };

  const options = semanticSurfaceOptions({
    expectedSides: { source: "bottom", target: "top" },
    relationship: { id: "r", relationshipType: "flow", flowId: "f", stepId: "s" },
    fromRect,
    toRect,
    blockerRects: [blockerOne, blockerTwo],
    canvasWidth: 1000,
    canvasHeight: 700
  });

  assert.ok(options.source.has("left"), "leftmost-column source escapes to the near (left) gutter");
  assert.ok(options.target.has("left"), "leftmost-column target escapes to the near (left) gutter");
  assert.equal(options.source.size > 1, true, "source keeps its facing surface plus the gutter escape");
});

test("vertical corridor blocked by a SINGLE intermediary keeps facing surfaces (no gutter detour)", () => {
  const fromRect = { x: 200, y: 0, width: 100, height: 50 };
  const toRect = { x: 200, y: 400, width: 100, height: 50 };
  const blocker = { x: 200, y: 180, width: 100, height: 50 };

  const options = semanticSurfaceOptions({
    expectedSides: { source: "bottom", target: "top" },
    relationship: { id: "r", relationshipType: "flow", flowId: "f", stepId: "s" },
    fromRect,
    toRect,
    blockerRects: [blocker],
    canvasWidth: 1000,
    canvasHeight: 500
  });

  assert.equal(options.source.size, 1, "a single intermediary is cheaper to jog around than to detour to the gutter");
});

// When the facing corridor is clear, no escape surfaces are added: the edge
// should use the facing surfaces directly.
test("clear horizontal corridor keeps only the facing surfaces", () => {
  const fromRect = { x: 0, y: 100, width: 100, height: 50 };
  const toRect = { x: 400, y: 100, width: 100, height: 50 };

  const options = semanticSurfaceOptions({
    expectedSides: { source: "right", target: "left" },
    relationship: { id: "r", relationshipType: "flow", flowId: "f", stepId: "s" },
    fromRect,
    toRect,
    blockerRects: [],
    canvasWidth: 600,
    canvasHeight: 300
  });

  assert.equal(options.source.size, 1, "no escape added when corridor is clear");
  assert.equal(options.target.size, 1, "no escape added when corridor is clear");
});
