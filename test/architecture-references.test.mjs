import assert from "node:assert/strict";
import test from "node:test";
import { validateArchitectureReferences } from "../src/domain/architecture-model/references.mjs";
import { releaseSummaryFromDetail } from "../src/domain/architecture-model/release-history.mjs";

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

test("release reference validation accepts index/detail item references", () => {
  const detail = {
    id: "v1-2-0",
    version: "1.2.0",
    name: "Release Truth",
    status: "active",
    posture: "on-track",
    summary: "Track release posture and history.",
    targetWindow: "next",
    lastUpdated: "2026-05-16T12:00:00.000Z",
    scope: {
      required: [{ id: "contract", title: "Contract", kind: "architecture", status: "complete", summary: "Define the release contract.", workstreamId: "data", dependsOn: [] }],
      planned: [{ id: "viewer", title: "Viewer", kind: "feature", status: "planned", summary: "Render release data.", workstreamId: "ui", dependsOn: ["contract"] }],
      stretch: [],
      deferred: [],
      outOfScope: []
    },
    workstreams: [
      { id: "data", name: "Data", owner: "maintainer", status: "complete", posture: "on-track", summary: "Data work.", itemIds: ["contract"], evidence: [] },
      { id: "ui", name: "UI", owner: "maintainer", status: "planned", posture: "on-track", summary: "UI work.", itemIds: ["viewer"], evidence: [] }
    ],
    blockers: [],
    milestones: [{ id: "slice", label: "Slice", status: "planned", order: 1, itemIds: ["contract", "viewer"] }],
    dependencies: [{ id: "viewer-after-contract", from: "viewer", to: "contract", summary: "Viewer follows contract." }],
    evidence: []
  };

  assert.deepEqual(validateArchitectureReferences(minimalModel({
    releases: {
      index: {
        currentReleaseId: "v1-2-0",
        releases: [releaseSummaryFromDetail(detail, "v1-2-0.json")]
      },
      details: [detail]
    }
  })), []);
});

test("release reference validation reports stale generated release history", () => {
  const detail = {
    id: "v1-2-0",
    version: "1.2.0",
    name: "Release Truth",
    status: "active",
    posture: "on-track",
    summary: "Track release posture and history.",
    targetWindow: "next",
    lastUpdated: "2026-05-16T12:00:00.000Z",
    scope: {
      required: [{ id: "contract", title: "Contract", kind: "architecture", status: "complete", summary: "Define the release contract.", dependsOn: [] }],
      planned: [],
      stretch: [],
      deferred: [],
      outOfScope: []
    },
    workstreams: [],
    blockers: [],
    milestones: [],
    dependencies: [],
    evidence: []
  };

  assert.deepEqual(validateArchitectureReferences(minimalModel({
    releases: {
      index: {
        currentReleaseId: "v1-2-0",
        releases: [{
          ...releaseSummaryFromDetail(detail, "v1-2-0.json"),
          counts: {
            features: 99,
            bugFixes: 0,
            workstreams: 0,
            blockers: 0,
            complete: 1,
            inProgress: 0,
            planned: 0,
            stretch: 0
          }
        }]
      },
      details: [detail]
    }
  })), [
    "release v1-2-0.index summary is stale; regenerate Release Truth history"
  ]);
});

test("release reference validation reports broken release relationships", () => {
  assert.deepEqual(validateArchitectureReferences(minimalModel({
    releases: {
      index: {
        currentReleaseId: "missing-release",
        releases: [{
          id: "v1-2-0",
          version: "1.2.0",
          status: "active"
        }]
      },
      details: [{
        id: "v1-2-0",
        version: "1.2.1",
        status: "released",
        scope: {
          required: [{ id: "contract", workstreamId: "missing-workstream", dependsOn: ["missing-item"] }],
          planned: [],
          stretch: [],
          deferred: [],
          outOfScope: []
        },
        workstreams: [{ id: "data", itemIds: ["missing-item"] }],
        blockers: [{ id: "blocker", itemIds: ["missing-item"] }],
        milestones: [{ id: "slice", itemIds: ["missing-item"] }],
        dependencies: [{ id: "broken-dependency", from: "contract", to: "missing-item" }]
      }]
    }
  })), [
    'releases.currentReleaseId references unknown id "missing-release"',
    "release v1-2-0.version does not match release index",
    "release v1-2-0.status does not match release index",
    "release v1-2-0.index summary is stale; regenerate Release Truth history",
    "release index v1-2-0 requires targetDate or targetWindow",
    "release v1-2-0.releasedAt is required for released entries",
    'release v1-2-0 item contract.workstreamId references unknown id "missing-workstream"',
    'release v1-2-0 item contract.dependsOn references unknown id "missing-item"',
    'release v1-2-0 workstream data.itemIds references unknown id "missing-item"',
    'release v1-2-0 blocker blocker.itemIds references unknown id "missing-item"',
    'release v1-2-0 milestone slice.itemIds references unknown id "missing-item"',
    'release v1-2-0 dependency broken-dependency.to references unknown id "missing-item"'
  ]);
});
