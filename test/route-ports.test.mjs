import assert from "node:assert/strict";
import test from "node:test";
import { routeEdges } from "../docs/architext/src/routing/routeEdges.js";

function routeInput(relationships) {
  return {
    relationships,
    visibleNodeIds: new Set(["a", "b", "c"]),
    nodeRects: new Map([
      ["a", { x: 0, y: 0, width: 100, height: 40 }],
      ["b", { x: 200, y: 0, width: 100, height: 40 }],
      ["c", { x: 200, y: 90, width: 100, height: 40 }]
    ]),
    laneIndexByNode: new Map([
      ["a", 0],
      ["b", 1],
      ["c", 1]
    ]),
    rowIndexByNode: new Map([
      ["a", 0],
      ["b", 0],
      ["c", 1]
    ]),
    canvasWidth: 420,
    canvasHeight: 220,
    marginY: 30,
    style: "orthogonal"
  };
}

test("single-surface routes prefer the surface centerpoint", () => {
  const routes = routeEdges(routeInput([
    { id: "single-a-b", from: "a", to: "b", label: "uses", relationshipType: "flow" }
  ]));
  const route = routes.get("single-a-b");

  assert.equal(route.points[0].y, 20);
  assert.equal(route.points.at(-1).y, 20);
});

test("fan-out routes may spread ports away from the centerpoint", () => {
  const routes = routeEdges(routeInput([
    { id: "fan-a-b", from: "a", to: "b", label: "uses", relationshipType: "flow" },
    { id: "fan-a-c", from: "a", to: "c", label: "uses", relationshipType: "flow" }
  ]));

  assert.notEqual(routes.get("fan-a-b").points[0].y, routes.get("fan-a-c").points[0].y);
});
