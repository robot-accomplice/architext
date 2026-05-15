import assert from "node:assert/strict";
import test from "node:test";
import { plannedCanvasFallback, planInputKey } from "../docs/architext/src/routing/usePlannedDiagram.js";

const input = {
  view: {
    id: "view",
    type: "system-map",
    lanes: [
      { id: "left", nodeIds: ["a"] },
      { id: "right", nodeIds: ["b", "c"] }
    ]
  },
  relationships: [{ id: "r", from: "a", to: "b", label: "uses", relationshipType: "flow", stepId: "s", flowId: "f" }],
  visibleNodeIds: new Set(["c", "a", "b"]),
  nodeWidth: 100,
  nodeHeight: 40,
  laneWidth: 180,
  rowGap: 90,
  marginX: 50,
  marginY: 70,
  minCanvasWidth: 300,
  minCanvasHeight: 200,
  canvasExtraWidth: 20,
  canvasExtraHeight: 30,
  style: "orthogonal"
};

test("planned diagram keys are stable when visible node set insertion order changes", () => {
  const reordered = { ...input, visibleNodeIds: new Set(["b", "a", "c"]) };
  assert.equal(planInputKey(input), planInputKey(reordered));
});

test("planned diagram fallback sizes from visible rows and layout constants", () => {
  assert.deepEqual(plannedCanvasFallback(input), {
    width: 480,
    height: 280
  });
});
