import assert from "node:assert/strict";
import test from "node:test";
import { surfaceSpacingCost } from "../viewer/src/routing/routeMountModel.js";
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
