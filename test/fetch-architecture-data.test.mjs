import assert from "node:assert/strict";
import test from "node:test";
import { loadArchitectureModel, loadReleaseDetail } from "../docs/architext/src/adapters/fetchArchitectureData.js";
import { releaseSummaryFromDetail } from "../src/domain/architecture-model/release-history.mjs";

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

test("architecture data adapter loads release index and current release detail", async () => {
  const previousRelease = {
    id: "v1-1-2",
    version: "1.1.2",
    name: "Previous",
    status: "released",
    posture: "shipped",
    summary: "Previous release.",
    releasedAt: "2026-05-16T09:20:00.000Z",
    lastUpdated: "2026-05-16T09:20:00.000Z",
    scope: { required: [], planned: [], stretch: [], deferred: [], outOfScope: [] },
    workstreams: [],
    blockers: [],
    milestones: [],
    dependencies: [],
    evidence: []
  };
  const currentRelease = {
    id: "v1-2-0",
    version: "1.2.0",
    name: "Current",
    status: "active",
    posture: "on-track",
    summary: "Current release.",
    targetWindow: "next",
    lastUpdated: "2026-05-16T12:00:00.000Z",
    scope: { required: [], planned: [], stretch: [], deferred: [], outOfScope: [] },
    workstreams: [],
    blockers: [],
    milestones: [],
    dependencies: [],
    evidence: []
  };
  const files = modelFiles({
    "/data/manifest.json": {
      defaultViewId: "main",
      files: {
        nodes: "nodes.json",
        flows: "flows.json",
        views: "views.json",
        dataClassification: "data-classification.json",
        decisions: "decisions.json",
        risks: "risks.json",
        releases: "releases/index.json"
      }
    },
    "/data/releases/index.json": {
      currentReleaseId: "v1-2-0",
      releases: [
        releaseSummaryFromDetail(previousRelease, "v1-1-2.json"),
        releaseSummaryFromDetail(currentRelease, "v1-2-0.json")
      ]
    },
    "/data/releases/v1-1-2.json": previousRelease,
    "/data/releases/v1-2-0.json": currentRelease
  });
  const requested = [];
  const fetcher = async (path) => {
    requested.push(path);
    return response(files[path]);
  };

  const model = await loadArchitectureModel(fetcher);
  const historicalDetail = await loadReleaseDetail(fetcher, model.releases, "v1-1-2");

  assert.equal(model.releases.index.currentReleaseId, "v1-2-0");
  assert.deepEqual(model.releases.details.map((detail) => detail.id), ["v1-2-0"]);
  assert.equal(historicalDetail.id, "v1-1-2");
  assert.equal(requested.includes("/data/releases/v1-1-2.json"), true);
});
