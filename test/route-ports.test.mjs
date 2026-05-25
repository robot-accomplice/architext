import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { planDiagram } from "../docs/architext/src/routing/planDiagram.js";
import { routeEdges } from "../docs/architext/src/routing/routeEdges.js";

const architextFlows = JSON.parse(readFileSync(new URL("../docs/architext/data/flows.json", import.meta.url), "utf8")).flows;
const architextViews = JSON.parse(readFileSync(new URL("../docs/architext/data/views.json", import.meta.url), "utf8")).views;

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

test("single routes on a node side use that side centerpoint", () => {
  const routes = routeEdges({
    relationships: [
      { id: "vertical", from: "above", to: "target", label: "writes", relationshipType: "flow" },
      { id: "horizontal", from: "left", to: "target", label: "reads", relationshipType: "flow" }
    ],
    visibleNodeIds: new Set(["above", "left", "target"]),
    nodeRects: new Map([
      ["above", { x: 200, y: 0, width: 100, height: 40 }],
      ["left", { x: 0, y: 110, width: 100, height: 40 }],
      ["target", { x: 200, y: 110, width: 100, height: 40 }]
    ]),
    laneIndexByNode: new Map([
      ["left", 0],
      ["above", 1],
      ["target", 1]
    ]),
    rowIndexByNode: new Map([
      ["above", 0],
      ["left", 1],
      ["target", 1]
    ]),
    canvasWidth: 420,
    canvasHeight: 220,
    marginY: 30,
    style: "orthogonal"
  });
  const vertical = routes.get("vertical");

  assert.equal(vertical.points[0].x, 250);
  assert.equal(vertical.points.at(-1).x, 250);
});

test("single top-side flow routes stay centered in the system map", () => {
  const view = architextViews.find((candidate) => candidate.name === "System Map");
  const flow = architextFlows.find((candidate) => candidate.name === "Fresh data-only install");
  const relationships = flow.steps.map((step, index) => ({
    id: step.id,
    from: step.from,
    to: step.to,
    label: `${index + 1}. ${step.action}`,
    relationshipType: "flow",
    stepId: step.id,
    flowId: flow.id,
    displayIndex: index + 1
  }));
  const plan = planDiagram({
    view,
    relationships,
    visibleNodeIds: new Set(view.lanes.flatMap((lane) => lane.nodeIds)),
    nodeWidth: 136,
    nodeHeight: 54,
    laneWidth: 210,
    rowGap: 102,
    marginX: 180,
    marginY: 76,
    minCanvasWidth: 900,
    minCanvasHeight: 640,
    canvasExtraWidth: 180,
    canvasExtraHeight: 160,
    style: "orthogonal"
  });
  const repositoryRect = plan.nodeRects.get("target-repository");
  const dataRect = plan.nodeRects.get("target-data-files");

  assert.deepEqual(plan.routes.get("write-metadata").points.at(-1), {
    x: repositoryRect.x + repositoryRect.width / 2,
    y: repositoryRect.y
  });
  assert.deepEqual(plan.routes.get("write-starter-data").points.at(-1), {
    x: dataRect.x + dataRect.width / 2,
    y: dataRect.y
  });
});
