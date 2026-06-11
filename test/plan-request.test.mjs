import assert from "node:assert/strict";
import test from "node:test";
import { buildFlowPlanRequest, buildFlowRelationships } from "../viewer/src/presentation/planRequest.js";
import { planInputKey } from "../viewer/src/routing/usePlannedDiagram.js";

// A flow with a decision step and two outcome branches, on a 3-lane view —
// exercises the decision rewrite (branch.from -> decision:<stepId>), preferred
// branch sides, and the diamond extra rects.
const view = {
  id: "v-plan-request",
  type: "flow-explorer",
  name: "Plan Request",
  lanes: [
    { id: "l0", name: "L0", nodeIds: ["entry"] },
    { id: "l1", name: "L1", nodeIds: ["decider"] },
    { id: "l2", name: "L2", nodeIds: ["accept", "reject"] }
  ]
};
const flow = {
  id: "f-plan-request",
  steps: [
    { id: "s1", from: "entry", to: "decider", action: "submit", summary: "in", data: [] },
    { id: "s2", from: "decider", to: "decider", action: "evaluate", summary: "gate", data: [], kind: "decision" },
    { id: "s3", from: "decider", to: "accept", action: "approve", summary: "yes", data: [], outcome: "valid" },
    { id: "s4", from: "decider", to: "reject", action: "deny", summary: "no", data: [], outcome: "invalid" }
  ]
};

test("buildFlowRelationships rewrites decision branches and numbers labels", () => {
  const relationships = buildFlowRelationships(flow, view);
  assert.equal(relationships.length, 4);
  const branch = relationships.find((r) => r.id === "s3");
  assert.equal(branch.from, "decision:s2", "outcome branch starts at the diamond");
  assert.equal(branch.componentFrom, "decider", "component identity preserved for selection");
  assert.equal(branch.stepId, "s2", "branch is attributed to the decision step");
  assert.equal(branch.branchStepId, "s3");
  assert.ok(branch.preferredStartSide, "branch carries a preferred start side");
  assert.match(relationships[0].label, /^1\. submit$/);
});

test("buildFlowPlanRequest is deterministic and carries diamond routing rects", () => {
  const first = buildFlowPlanRequest({ view, flow, layoutConfig: undefined, style: "orthogonal" });
  const second = buildFlowPlanRequest({ view, flow, layoutConfig: undefined, style: "orthogonal" });
  assert.equal(planInputKey(first.planInput), planInputKey(second.planInput), "same inputs -> same canonical key");
  assert.equal(first.decisionNodes.length, 1);
  const rect = first.planInput.extraNodeRects.get("decision:s2");
  assert.ok(rect, "diamond rect registered as an extra node rect");
  assert.equal(rect.fixedPorts, true);
  assert.ok(rect.sideAnchors.top && rect.sideAnchors.bottom, "diamond exposes tip anchors");
  assert.ok(first.planInput.visibleNodeIds.has("decider"));
  assert.equal(first.planInput.style, "orthogonal");
});

test("layout config participates in the key (config change -> different key)", () => {
  const base = buildFlowPlanRequest({ view, flow, layoutConfig: undefined, style: "orthogonal" });
  const widened = buildFlowPlanRequest({ view, flow, layoutConfig: { laneWidth: 400 }, style: "orthogonal" });
  assert.notEqual(planInputKey(base.planInput), planInputKey(widened.planInput));
});
