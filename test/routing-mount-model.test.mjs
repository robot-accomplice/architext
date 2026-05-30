import assert from "node:assert/strict";
import test from "node:test";
import { surfaceSpacingCost, mountAssignmentCost, applyOffsetWithMatch } from "../viewer/src/routing/routeMountModel.js";
import { MIN_LEGIBLE_GAP, MOUNT_COST } from "../viewer/src/routing/routeConstants.js";

// A left/right surface of length 54 with 3 mounts: ideal slots at 54*[1,2,3]/4.
test("evenly-spread mounts cost less than crammed mounts", () => {
  const length = 54;
  const ideal = [1, 2, 3].map((i) => (i / 4) * length); // 13.5, 27, 40.5
  const crammed = [26, 27, 28];                          // ~1px gaps
  assert.ok(surfaceSpacingCost(ideal, length, 3) < surfaceSpacingCost(crammed, length, 3));
});

test("a gap below MIN_LEGIBLE_GAP is penalized as cramped", () => {
  const length = 54;
  const wide = [10, 27, 44];        // gaps 17 — all >= MIN_LEGIBLE_GAP
  const tight = [25, 27, 29];       // gaps 2 — below MIN_LEGIBLE_GAP
  assert.equal(surfaceSpacingCost(wide, length, 3) < surfaceSpacingCost(tight, length, 3), true);
  assert.ok(MIN_LEGIBLE_GAP > 2);
});

test("a route through a non-endpoint node body costs at least one tier-0 collision unit", () => {
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 40 }],
    ["b", { x: 400, y: 0, width: 40, height: 40 }],
    ["mid", { x: 180, y: 0, width: 40, height: 40 }]   // sits in the straight corridor a->b
  ]);
  const input = { nodeRects, visibleNodeIds: new Set(["a", "b", "mid"]), canvasWidth: 480, canvasHeight: 60 };
  const relationshipById = new Map([["e", { id: "e", from: "a", to: "b", relationshipType: "flow" }]]);
  const clean = new Map([["e", { style: "orthogonal", points: [{ x: 40, y: 20 }, { x: 100, y: 20 }, { x: 100, y: -40 }, { x: 380, y: -40 }, { x: 380, y: 20 }, { x: 400, y: 20 }], bends: 4, samples: [] }]]);
  const through = new Map([["e", { style: "orthogonal", points: [{ x: 40, y: 20 }, { x: 400, y: 20 }], bends: 4, samples: [] }]]); // straight line crosses "mid"; same bends so only collision differs
  const c = (r) => mountAssignmentCost(r, relationshipById, input);
  // The straight route plows through "mid" (a non-endpoint node) -> tier-0 collision;
  // the detour avoids every node body. Tier-0 must dominate the detour's extra bends.
  assert.ok(c(through) - c(clean) >= MOUNT_COST.collision, `expected tier-0 gap, got ${c(through) - c(clean)}`);
});

test("co-shifting a straight facing edge's partner keeps it straight", () => {
  // a.right at y=20 -> b.left at y=20, straight horizontal edge.
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 60 }],
    ["b", { x: 200, y: 0, width: 40, height: 60 }]
  ]);
  const input = { nodeRects };
  const routeById = new Map([["e", { points: [{ x: 40, y: 20 }, { x: 200, y: 20 }], bends: 0 }]]);
  const relationshipById = new Map([["e", { id: "e", from: "a", to: "b", relationshipType: "flow" }]]);
  // Move a.right mount down by +10 (to y=30); partner b.left must follow to stay straight.
  applyOffsetWithMatch(routeById, relationshipById, input, { id: "e", endpointIndex: 0, side: "right", rect: nodeRects.get("a") }, 10);
  const pts = routeById.get("e").points;
  assert.equal(pts[0].y, pts[pts.length - 1].y, "edge stays straight (both ends moved together)");
  assert.equal(pts[0].y, 30);
});
