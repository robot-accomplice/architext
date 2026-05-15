import assert from "node:assert/strict";
import test from "node:test";
import { loadArchitectureModel } from "../docs/architext/src/adapters/fetchArchitectureData.js";

function response(body, ok = true) {
  return {
    ok,
    status: ok ? 200 : 404,
    statusText: ok ? "OK" : "Not Found",
    json: async () => body
  };
}

function modelFiles(overrides = {}) {
  return {
    "/data/manifest.json": {
      defaultViewId: "main",
      files: {
        nodes: "nodes.json",
        flows: "flows.json",
        views: "views.json",
        dataClassification: "data-classification.json",
        decisions: "decisions.json",
        risks: "risks.json"
      }
    },
    "/data/nodes.json": {
      nodes: [{
        id: "actor",
        dependencies: [],
        dataHandled: [],
        relatedFlows: ["flow"],
        relatedDecisions: [],
        knownRisks: []
      }]
    },
    "/data/flows.json": {
      flows: [{ id: "flow", actors: ["actor"], steps: [{ id: "step", from: "actor", to: "actor", data: [] }] }]
    },
    "/data/views.json": { views: [{ id: "main", lanes: [{ id: "lane", nodeIds: ["actor"] }] }] },
    "/data/data-classification.json": { classes: [] },
    "/data/decisions.json": { decisions: [] },
    "/data/risks.json": { risks: [] },
    ...overrides
  };
}

test("architecture data adapter loads the manifest file graph", async () => {
  const files = modelFiles();
  const requested = [];
  const model = await loadArchitectureModel(async (path) => {
    requested.push(path);
    return response(files[path]);
  });

  assert.deepEqual(requested.sort(), Object.keys(files).sort());
  assert.equal(model.manifest.defaultViewId, "main");
  assert.equal(model.nodes[0].id, "actor");
});

test("architecture data adapter applies shared reference validation", async () => {
  const files = modelFiles({
    "/data/views.json": { views: [{ id: "main", lanes: [{ id: "lane", nodeIds: ["missing-node"] }] }] }
  });

  await assert.rejects(
    loadArchitectureModel(async (path) => response(files[path])),
    /view main lane lane references unknown id "missing-node"/
  );
});
