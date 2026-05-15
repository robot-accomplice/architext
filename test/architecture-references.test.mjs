import assert from "node:assert/strict";
import test from "node:test";
import { validateArchitectureReferences } from "../src/domain/architecture-model/references.mjs";

function minimalModel(overrides = {}) {
  return {
    manifest: { defaultViewId: "main", ...(overrides.manifest ?? {}) },
    nodes: [
      {
        id: "actor",
        dependencies: ["service"],
        dataHandled: ["data"],
        relatedFlows: ["flow"],
        relatedDecisions: ["decision"],
        knownRisks: ["risk"]
      },
      {
        id: "service",
        dependencies: [],
        dataHandled: [],
        relatedFlows: [],
        relatedDecisions: [],
        knownRisks: []
      }
    ],
    flows: [{ id: "flow", actors: ["actor"], steps: [{ id: "step", from: "actor", to: "service", data: ["data"] }] }],
    views: [{ id: "main", lanes: [{ id: "lane", nodeIds: ["actor", "service"] }] }],
    dataClasses: [{ id: "data" }],
    decisions: [{ id: "decision" }],
    risks: [{ id: "risk" }],
    ...overrides
  };
}

test("architecture reference validation accepts a closed model graph", () => {
  assert.deepEqual(validateArchitectureReferences(minimalModel()), []);
});

test("architecture reference validation reports unknown ids with context", () => {
  assert.deepEqual(validateArchitectureReferences(minimalModel({
    manifest: { defaultViewId: "missing-view" },
    flows: [{ id: "flow", actors: ["missing-actor"], steps: [{ id: "step", from: "actor", to: "missing-node", data: ["missing-data"] }] }],
    views: [{ id: "main", lanes: [{ id: "lane", nodeIds: ["missing-node"] }] }]
  })), [
    'manifest.defaultViewId references unknown id "missing-view"',
    'flow flow.actors references unknown id "missing-actor"',
    'flow flow step step.to references unknown id "missing-node"',
    'flow flow step step.data references unknown id "missing-data"',
    'view main lane lane references unknown id "missing-node"'
  ]);
});
