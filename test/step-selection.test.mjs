import assert from "node:assert/strict";
import test from "node:test";
import {
  isSelectedStep,
  orderSelectedLast,
  selectedFlowIdForSelection,
  selectedStepIdForSelection
} from "../docs/architext/src/presentation/stepSelection.js";

test("step selection is scoped to the selected flow", () => {
  const selection = { kind: "step", flowId: "flow-a", stepId: "step-2" };

  assert.equal(selectedStepIdForSelection(selection), "step-2");
  assert.equal(selectedFlowIdForSelection(selection), "flow-a");
  assert.equal(isSelectedStep(selection, "flow-a", "step-2"), true);
  assert.equal(isSelectedStep(selection, "flow-b", "step-2"), false);
});

test("relationship selection carries ordered step identity", () => {
  const selection = { kind: "relationship", flowId: "flow-a", stepId: "step-3" };

  assert.equal(isSelectedStep(selection, "flow-a", "step-3"), true);
  assert.equal(isSelectedStep(selection, "flow-a", "step-4"), false);
});

test("selected ordered steps render last so the highlighted route stays visible", () => {
  const steps = [
    { id: "step-1" },
    { id: "step-2" },
    { id: "step-3" }
  ];

  assert.deepEqual(orderSelectedLast(steps, (step) => step.id === "step-2").map((step) => step.id), [
    "step-1",
    "step-3",
    "step-2"
  ]);
});
