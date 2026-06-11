import assert from "node:assert/strict";
import test from "node:test";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";
import { buildFlowPlanRequest } from "../viewer/src/presentation/planRequest.js";
import { serializePlan, deserializePlan } from "../viewer/src/routing/planCodec.js";

const view = {
  id: "v-codec",
  type: "flow-explorer",
  name: "Codec",
  lanes: [
    { id: "l0", name: "a", nodeIds: ["ca"] },
    { id: "l1", name: "b", nodeIds: ["cb", "cc"] }
  ]
};
const flow = {
  id: "f-codec",
  steps: [
    { id: "s1", from: "ca", to: "cb", action: "go", summary: "", data: [] },
    { id: "s2", from: "cb", to: "cc", action: "next", summary: "", data: [] }
  ]
};

test("plan survives a JSON round-trip exactly (modulo positionFor)", () => {
  const { planInput } = buildFlowPlanRequest({ view, flow, layoutConfig: undefined, style: "orthogonal" });
  const plan = planDiagram(planInput);

  const wire = JSON.parse(JSON.stringify(serializePlan(plan)));
  const restored = deserializePlan(wire);

  assert.equal(restored.canvasWidth, plan.canvasWidth);
  assert.deepEqual(Array.from(restored.visibleNodeIds).sort(), Array.from(plan.visibleNodeIds).sort());
  assert.deepEqual(Array.from(restored.nodeRects.entries()), Array.from(plan.nodeRects.entries()));
  assert.equal(restored.routes.size, plan.routes.size);
  for (const [id, route] of plan.routes) {
    assert.deepEqual(restored.routes.get(id), JSON.parse(JSON.stringify(route)), `route ${id} identical over the wire`);
  }
  assert.deepEqual(Array.from(restored.labelBoxes.keys()), Array.from(plan.labelBoxes.keys()));
});

test("codec fails loudly on unexpected shapes", () => {
  assert.throws(() => serializePlan({ routes: [] }), /expected Map/);
  assert.throws(() => deserializePlan({ routes: {} }), /expected entries array/);
});
