import assert from "node:assert/strict";
import test from "node:test";
import { diagramLayoutFor } from "../viewer/src/presentation/diagramLayout.js";

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

test("diagram layout treats relationship-heavy flow explorers as dense", () => {
  const denseFlowExplorer = {
    id: "agent-turn-flow",
    type: "flow-explorer",
    lanes: [
      { id: "entry", nodeIds: ["operator", "browser", "socket", "channel"] },
      { id: "runtime", nodeIds: ["pipeline"] },
      { id: "services", nodeIds: ["memory", "tools", "store", "model"] }
    ]
  };
  const simpleFlowExplorer = {
    id: "simple-flow",
    type: "flow-explorer",
    lanes: [
      { id: "left", nodeIds: ["a"] },
      { id: "right", nodeIds: ["b"] }
    ]
  };

  const dense = diagramLayoutFor(denseFlowExplorer, 18);
  const simple = diagramLayoutFor(simpleFlowExplorer, 1);

  assert.ok(dense.rowGap > simple.rowGap);
  assert.ok(dense.laneWidth > simple.laneWidth);
  assert.ok(dense.routeGutter > simple.routeGutter);
});

const simpleView = {
  id: "cfg",
  type: "system-map",
  lanes: [
    { id: "left", nodeIds: ["a"] },
    { id: "right", nodeIds: ["b"] }
  ]
};

test("a null layout config renders byte-identically to no config", () => {
  // The whole feature rests on this: an absent config must not shift any
  // diagram. If this breaks, every existing diagram silently moves.
  assert.deepEqual(diagramLayoutFor(simpleView, 3, null), diagramLayoutFor(simpleView, 3));
});

test("layout overrides apply and an overridden routeGutter cascades to derived margins", () => {
  const layout = diagramLayoutFor(simpleView, 3, { laneWidth: 400, routeGutter: 200 });
  assert.equal(layout.laneWidth, 400);
  assert.equal(layout.routeGutter, 200);
  assert.equal(layout.marginX, 248); // routeGutter + 48, recomputed from the override
  assert.equal(layout.canvasExtraWidth, 200); // tracks the overridden routeGutter
  assert.equal(layout.nodeWidth, 136); // unspecified fields keep their default
});

test("a user override wins over the dense auto-scaled value", () => {
  const denseDeployment = {
    id: "dense",
    type: "deployment",
    lanes: [{ id: "l", nodeIds: ["a", "b", "c", "d", "e"] }, { id: "r", nodeIds: ["f"] }]
  };
  const autoDense = diagramLayoutFor(denseDeployment, 10);
  assert.equal(autoDense.laneWidth, 240); // dense default
  const overridden = diagramLayoutFor(denseDeployment, 10, { laneWidth: 300 });
  assert.equal(overridden.laneWidth, 300); // explicit override beats dense auto-scale
});

test("diagram layout reserves room below lane headings", () => {
  const layout = diagramLayoutFor({
    id: "flow",
    type: "flow-explorer",
    lanes: [
      { id: "one", nodeIds: ["a"] },
      { id: "two", nodeIds: ["b"] }
    ]
  }, 1);

  assert.ok(layout.marginY >= 104);
});
