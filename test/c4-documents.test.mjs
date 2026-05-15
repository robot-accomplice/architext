import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { c4LayoutFor } from "../docs/architext/src/routing/c4Layout.js";
import { planDiagram } from "../docs/architext/src/routing/planDiagram.js";
import { relationshipLabel } from "../docs/architext/src/routing/relationshipLabels.js";
import { routeIntersectsRect } from "../docs/architext/src/routing/routeEdges.js";

const architextDataDir = path.resolve(import.meta.dirname, "../docs/architext/data");

const c4TypeExpectations = {
  "c4-context": new Set(["actor", "client", "service", "deployment-unit", "external-service", "software-system", "trust-boundary"]),
  "c4-container": new Set(["actor", "client", "service", "worker", "data-store", "queue", "deployment-unit", "external-service", "software-system"]),
  "c4-component": new Set(["actor", "client", "service", "module", "worker", "data-store", "queue", "external-service"])
};

const c4DensityBudgets = {
  "c4-context": { nodes: 14, relationships: 18 },
  "c4-container": { nodes: 14, relationships: 24 },
  "c4-component": { nodes: 14, relationships: 28 }
};

function readJson(dataDir, name) {
  return JSON.parse(readFileSync(path.join(dataDir, name), "utf8"));
}

function c4Views(dataDir) {
  return readJson(dataDir, "views.json").views.filter((view) => view.type.startsWith("c4-"));
}

function nodesById(dataDir) {
  return new Map(readJson(dataDir, "nodes.json").nodes.map((node) => [node.id, node]));
}

function duplicateNodeIds(view) {
  const counts = new Map();
  for (const nodeId of view.lanes.flatMap((lane) => lane.nodeIds)) {
    counts.set(nodeId, (counts.get(nodeId) ?? 0) + 1);
  }
  return [...counts].filter(([, count]) => count > 1).map(([nodeId]) => nodeId);
}

function structuralRelationships(view, nodeMap) {
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  return [...visibleNodeIds].flatMap((nodeId) => {
    const node = nodeMap.get(nodeId);
    return (node?.dependencies ?? [])
      .filter((dependencyId) => visibleNodeIds.has(dependencyId))
      .map((dependencyId) => {
        const target = nodeMap.get(dependencyId);
        const label = relationshipLabel(node, target);
        return {
          id: `${nodeId}-${dependencyId}`,
          from: nodeId,
          to: dependencyId,
          label,
          relationshipType: "structural"
        };
      });
  });
}

function assertC4DocumentQuality(dataDir, fixtureName) {
  const nodeMap = nodesById(dataDir);
  for (const view of c4Views(dataDir)) {
    const relationships = structuralRelationships(view, nodeMap);
    const nodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
    const budget = c4DensityBudgets[view.type];
    assert.deepEqual(duplicateNodeIds(view), [], `${fixtureName}:${view.id} duplicates node membership`);
    const allowedTypes = c4TypeExpectations[view.type];
    assert.ok(allowedTypes, `${fixtureName}:${view.id} has unsupported C4 type ${view.type}`);
    assert.ok(nodeIds.size <= budget.nodes, `${fixtureName}:${view.id} has ${nodeIds.size} nodes; split dense ${view.type} views before tuning routes`);
    assert.ok(relationships.length <= budget.relationships, `${fixtureName}:${view.id} has ${relationships.length} relationships; split dense ${view.type} views before tuning routes`);
    for (const nodeId of view.lanes.flatMap((lane) => lane.nodeIds)) {
      const node = nodeMap.get(nodeId);
      assert.ok(node, `${fixtureName}:${view.id} references missing node ${nodeId}`);
      assert.ok(allowedTypes.has(node.type), `${fixtureName}:${view.id} includes ${nodeId} with ${node.type}`);
    }
    for (const relationship of relationships) {
      assert.equal(relationship.relationshipType, "structural");
      assert.doesNotMatch(relationship.id, /:/, `${fixtureName}:${view.id} should not use flow-step relationships`);
      assert.doesNotMatch(relationship.label, /^\d+\./, `${fixtureName}:${view.id} should not use numbered flow labels`);
    }
  }
}

function assertC4RouteFitness(dataDir, fixtureName) {
  const nodeMap = nodesById(dataDir);
  const reports = [];

  for (const view of c4Views(dataDir)) {
    const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
    const relationships = structuralRelationships(view, nodeMap);

    for (const style of ["orthogonal", "curved"]) {
      const plan = planDiagram({
        view,
        relationships,
        visibleNodeIds,
        ...c4LayoutFor(view.type),
        style
      });
      let collisions = 0;
      for (const relationship of relationships) {
        const route = plan.routes.get(relationship.id);
        assert.ok(route, `missing route for ${view.id}:${relationship.id}`);
        for (const [nodeId, rect] of plan.nodeRects) {
          if (nodeId === relationship.from || nodeId === relationship.to) continue;
          if (routeIntersectsRect(route, rect, 0)) {
            collisions += 1;
            break;
          }
        }
      }

      reports.push(`${view.id}:${style}:warnings=${plan.warnings.length}:collisions=${collisions}`);
      assert.equal(collisions, 0, `${fixtureName}: ${reports.join(", ")}`);
      assert.equal(plan.warnings.length, 0, `${fixtureName}: ${reports.join(", ")}`);
    }
  }
}

test("Architext C4 documents follow document quality gates", () => {
  const views = c4Views(architextDataDir);
  assert.ok(views.some((view) => view.type === "c4-context"), "Architext needs a C4 Context view");
  assert.ok(views.some((view) => view.type === "c4-container"), "Architext needs C4 Container views");
  assert.ok(views.some((view) => view.type === "c4-component"), "Architext needs a C4 Component view");
  assertC4DocumentQuality(architextDataDir, "Architext");
});

test("Architext C4 routes have no collisions or warnings", () => {
  assertC4RouteFitness(architextDataDir, "Architext");
});
