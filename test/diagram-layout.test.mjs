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
