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

test("planned diagram keys differ when extra node rects change", () => {
  const withExtra = {
    ...input,
    extraNodeRects: new Map([["d", { x: 10, y: 20, width: 30, height: 40 }]])
  };
  const moved = {
    ...input,
    extraNodeRects: new Map([["d", { x: 99, y: 20, width: 30, height: 40 }]])
  };
  assert.notEqual(planInputKey(withExtra), planInputKey(moved));
});

test("planned diagram keys differ when extra lane index changes", () => {
  const withLane = {
    ...input,
    extraLaneIndexByNode: new Map([["d", 0]])
  };
  const otherLane = {
    ...input,
    extraLaneIndexByNode: new Map([["d", 1]])
  };
  assert.notEqual(planInputKey(withLane), planInputKey(otherLane));
});

test("planned diagram keys differ when extra row index changes", () => {
  const withRow = {
    ...input,
    extraRowIndexByNode: new Map([["d", 0]])
  };
  const otherRow = {
    ...input,
    extraRowIndexByNode: new Map([["d", 2]])
  };
  assert.notEqual(planInputKey(withRow), planInputKey(otherRow));
});

test("planned diagram keys differ when edge proximity scoring toggles", () => {
  const off = { ...input, scoreEdgeProximity: false };
  const on = { ...input, scoreEdgeProximity: true };
  assert.notEqual(planInputKey(off), planInputKey(on));
});

test("planned diagram keys are stable when extra node rect key order changes", () => {
  const a = {
    ...input,
    extraNodeRects: new Map([
      ["d", { x: 10, y: 20, width: 30, height: 40 }],
      ["e", { x: 50, y: 60, width: 30, height: 40 }]
    ])
  };
  const b = {
    ...input,
    extraNodeRects: new Map([
      ["e", { x: 50, y: 60, width: 30, height: 40 }],
      ["d", { x: 10, y: 20, width: 30, height: 40 }]
    ])
  };
  assert.equal(planInputKey(a), planInputKey(b));
});

test("planned diagram fallback sizes from visible rows and layout constants", () => {
  assert.deepEqual(plannedCanvasFallback(input), {
    width: 480,
    height: 280
  });
});
