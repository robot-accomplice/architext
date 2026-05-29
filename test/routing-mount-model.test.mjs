import assert from "node:assert/strict";
import test from "node:test";
import { surfaceSpacingCost, mountAssignmentCost } from "../viewer/src/routing/routeMountModel.js";
import { MIN_LEGIBLE_GAP } from "../viewer/src/routing/routeConstants.js";

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

// Two facing nodes, one straight edge; verify a clean straight route costs less
// than the same edge forced through a node body (collision) — tier 0 dominates.
function twoNodeInput() {
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 40 }],
    ["b", { x: 200, y: 0, width: 40, height: 40 }]
  ]);
  return { nodeRects, visibleNodeIds: new Set(["a", "b"]), canvasWidth: 280, canvasHeight: 60 };
}

test("a colliding assignment costs at least one tier-0 unit more than a clean one", () => {
  const input = twoNodeInput();
  const clean = new Map([["e", { points: [{ x: 40, y: 20 }, { x: 200, y: 20 }], bends: 0 }]]);
  const relationshipById = new Map([["e", { id: "e", from: "a", to: "b", relationshipType: "flow" }]]);
  const colliding = new Map([["e", { points: [{ x: 40, y: 20 }, { x: 120, y: 20 }, { x: 120, y: -200 }, { x: 200, y: 20 }], bends: 2 }]]);
  assert.ok(mountAssignmentCost(clean, relationshipById, input) < mountAssignmentCost(colliding, relationshipById, input));
});
