import assert from "node:assert/strict";
import test from "node:test";
import { diagramLayoutFor } from "../docs/architext/src/presentation/diagramLayout.js";

test("diagram layout expands dense topology views before routing", () => {
  const denseDeployment = {
    id: "dense-deployment",
    type: "deployment",
    lanes: [
      { id: "left", nodeIds: ["a", "b", "c", "d", "e"] },
      { id: "right", nodeIds: ["f", "g", "h"] }
    ]
  };
  const simpleSystemMap = {
    id: "simple-system",
    type: "system-map",
    lanes: [
      { id: "left", nodeIds: ["a"] },
      { id: "right", nodeIds: ["b"] }
    ]
  };

  const dense = diagramLayoutFor(denseDeployment, 10);
  const simple = diagramLayoutFor(simpleSystemMap, 10);

  assert.ok(dense.rowGap > simple.rowGap);
  assert.ok(dense.laneWidth > simple.laneWidth);
  assert.ok(dense.canvasExtraWidth > simple.canvasExtraWidth);
});
