import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { planDiagram } from "../docs/architext/src/routing/planDiagram.js";
import { relationshipLabel } from "../docs/architext/src/routing/relationshipLabels.js";
import { routeIntersectsRect } from "../docs/architext/src/routing/routeEdges.js";

const dataDir = path.resolve(import.meta.dirname, "../../roboticus/docs/architext/data");
const hasRoboticus = existsSync(path.join(dataDir, "manifest.json"));

function readJson(name) {
  return JSON.parse(readFileSync(path.join(dataDir, name), "utf8"));
}

test("Roboticus non-C4 diagram routes avoid non-endpoint node bodies", { skip: hasRoboticus ? false : "Roboticus repo is not checked out next to Architext" }, () => {
  const { nodes } = readJson("nodes.json");
  const { flows } = readJson("flows.json");
  const { views } = readJson("views.json");
  const nodesById = new Map(nodes.map((node) => [node.id, node]));
  const diagramViewTypes = new Set(["system-map", "flow-explorer", "dataflow", "deployment", "risk-overlay"]);
  const reports = [];

  for (const view of views.filter((item) => diagramViewTypes.has(item.type))) {
    const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));

    const structuralRelationships = Array.from(visibleNodeIds).flatMap((nodeId) => {
      const node = nodesById.get(nodeId);
      return (node?.dependencies ?? [])
        .filter((dependencyId) => visibleNodeIds.has(dependencyId))
        .map((dependencyId) => ({
          id: `${nodeId}-${dependencyId}`,
          from: nodeId,
          to: dependencyId,
          label: relationshipLabel(node, nodesById.get(dependencyId))
        }));
    });
    const flowRelationships = flows.flatMap((flow) => flow.steps
      .filter((step) => visibleNodeIds.has(step.from) && visibleNodeIds.has(step.to))
      .map((step, index) => ({
        id: `${flow.id}:${step.id}`,
        from: step.from,
        to: step.to,
        label: `${index + 1}. ${step.action}`
      })));

    for (const style of ["orthogonal", "curved"]) {
      for (const [kind, relationships] of [["structural", structuralRelationships], ["flow", flowRelationships]]) {
        const plan = planDiagram({
          view,
          relationships,
          visibleNodeIds,
          nodeWidth: 136,
          nodeHeight: 54,
          laneWidth: 210,
          rowGap: 102,
          marginX: 180,
          marginY: 76,
          minCanvasWidth: 0,
          minCanvasHeight: 340,
          canvasExtraWidth: 132,
          canvasExtraHeight: 88,
          style
        });
        let collisions = 0;
        for (const relationship of relationships) {
          const route = plan.routes.get(relationship.id);
          assert.ok(route, `missing route for ${relationship.id}`);
          assert.equal(route.style, style);
          for (const [nodeId, rect] of plan.nodeRects) {
            if (nodeId === relationship.from || nodeId === relationship.to) continue;
            if (routeIntersectsRect(route, rect, 0)) {
              collisions += 1;
              break;
            }
          }
        }
        reports.push(`${view.id}:${style}:${kind}:${collisions}`);
        assert.equal(collisions, 0, reports.join(", "));
      }
    }
  }
});
