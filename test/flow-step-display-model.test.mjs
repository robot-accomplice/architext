import assert from "node:assert/strict";
import test from "node:test";
import {
  decisionBranchTargets,
  flowStepDisplayIndexes,
  isDecisionBranchSupportStep,
  isRenderedFlowRelationshipStep
} from "../docs/architext/src/presentation/flowStepDisplayModel.js";

test("flow step display indexes reuse the decision number for outgoing outcome branches", () => {
  const indexes = flowStepDisplayIndexes([
    { id: "load", from: "cli", to: "data", action: "load", summary: "", data: [] },
    { id: "decide", from: "cli", to: "validator", kind: "decision", action: "isValid", summary: "", data: [] },
    { id: "valid", from: "validator", to: "viewer", outcome: "valid", action: "render", summary: "", data: [] },
    { id: "invalid", from: "validator", to: "cli", outcome: "invalid", action: "report", summary: "", data: [] },
    { id: "finish", from: "viewer", to: "user", action: "finish", summary: "", data: [] }
  ]);

  assert.equal(indexes.get("load"), 1);
  assert.equal(indexes.get("decide"), 2);
  assert.equal(indexes.get("valid"), 2);
  assert.equal(indexes.get("invalid"), 2);
  assert.equal(indexes.get("finish"), 5);
});

test("flow step display model separates branched decisions from component routes", () => {
  const steps = [
    { id: "decide", from: "cli", to: "validator", kind: "decision", action: "isValid", summary: "", data: [] },
    { id: "valid", from: "validator", to: "viewer", outcome: "valid", action: "render", summary: "", data: [] },
    { id: "invalid", from: "validator", to: "cli", outcome: "invalid", action: "report", summary: "", data: [] }
  ];

  assert.deepEqual([...decisionBranchTargets(steps)], ["validator"]);
  assert.equal(isRenderedFlowRelationshipStep(steps, steps[0], 0), true);
  assert.equal(isRenderedFlowRelationshipStep(steps, steps[1], 1), true);
  assert.equal(isRenderedFlowRelationshipStep(steps, steps[2], 2), true);
  assert.equal(isDecisionBranchSupportStep(steps, steps[1], 1), true);
  assert.equal(isDecisionBranchSupportStep(steps, steps[2], 2), true);
});
