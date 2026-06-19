import assert from "node:assert/strict";
import test from "node:test";
import {
  nodeLanePosition,
  oppositeSide,
  preferredDecisionBranchSide,
  preferredDecisionBranchEndSide
} from "../viewer/src/presentation/decisionBranchModel.js";

// Mirrors the feature-showcase: decision at lane 1, branch targets in lane 2.
const view = {
  lanes: [
    { nodeIds: ["n90", "n91"] },
    { nodeIds: ["n92"] },
    { nodeIds: ["n93", "n94"] }
  ]
};
const decisionAtN92 = nodeLanePosition(view, "n92"); // { laneIndex: 1, rowIndex: 0 }

test("oppositeSide returns the facing side", () => {
  assert.equal(oppositeSide("left"), "right");
  assert.equal(oppositeSide("right"), "left");
  assert.equal(oppositeSide("top"), "bottom");
  assert.equal(oppositeSide("bottom"), "top");
});

test("forward branches FAN off different tips so siblings don't share one exit", () => {
  // n93 is on the decision's row -> right tip; n94 is below -> bottom tip.
  // (Both leaving the right tip made them share a segment and dogleg.)
  assert.equal(preferredDecisionBranchSide(view, decisionAtN92, "n93"), "right");
  assert.equal(preferredDecisionBranchSide(view, decisionAtN92, "n94"), "bottom");
});

test("a forward branch ENTERS its target on the FACING side, never the far side", () => {
  // Regression for the decision-branch overshoot: leaving the diamond rightward
  // toward a target in the next column, the branch must mount the target's LEFT
  // (facing) face. Mounting the right face forces the route to wrap PAST the node
  // and double back (the 36px overshoot seen in the rendered feature-showcase).
  assert.equal(
    preferredDecisionBranchEndSide(view, decisionAtN92, "n93", "right"),
    "left",
    "forward-column branch must enter the facing (left) face"
  );
  assert.equal(
    preferredDecisionBranchEndSide(view, decisionAtN92, "n94", "right"),
    "left"
  );
});

test("the end side faces the start side for both forward and backward branches", () => {
  // Backward branch (target in a left column): leaves on the left, enters facing right.
  const backwardView = {
    lanes: [{ nodeIds: ["a"] }, { nodeIds: ["d"] }]
  };
  const decisionAtD = { laneIndex: 1, rowIndex: 0 };
  const startLeft = preferredDecisionBranchSide(backwardView, decisionAtD, "a");
  assert.equal(startLeft, "left");
  assert.equal(preferredDecisionBranchEndSide(backwardView, decisionAtD, "a", startLeft), "right");
});

test("a diamond supports up to 3 branches — one per point NOT facing the node", () => {
  // The diamond sits below its node, so its TOP point faces the node and is
  // reserved; the other 3 points (right/bottom/left) each carry one branch.
  // A 3-outcome decision with targets in 3 directions gets 3 DISTINCT points.
  const threeWay = {
    lanes: [
      { nodeIds: ["back"] },          // lane 0 (a backward target)
      { nodeIds: ["up", "gate", "down"] }, // lane 1: decision = "gate" (row 1)
      { nodeIds: ["fwd"] }            // lane 2 (a forward target)
    ]
  };
  const gate = nodeLanePosition(threeWay, "gate"); // { laneIndex: 1, rowIndex: 1 }
  const sides = ["fwd", "down", "back"].map((t) => preferredDecisionBranchSide(threeWay, gate, t));
  // never the node-facing (top) point
  assert.ok(!sides.includes("top"), "no branch uses the node-facing (top) point");
  // three distinct points for the three branches
  assert.equal(new Set(sides).size, 3, `expected 3 distinct branch points, got ${sides.join(",")}`);
});

test("same-lane branches keep their dedicated end sides (unchanged)", () => {
  const stackView = { lanes: [{ nodeIds: ["above", "decision", "below"] }] };
  const decisionPos = { laneIndex: 0, rowIndex: 1 };
  assert.equal(preferredDecisionBranchEndSide(stackView, decisionPos, "above", "left"), "left");
  assert.equal(preferredDecisionBranchEndSide(stackView, decisionPos, "below", "bottom"), "top");
});
