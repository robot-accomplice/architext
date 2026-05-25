import assert from "node:assert/strict";
import test from "node:test";
import {
  compatibleFlowsForView,
  compatibleFlowViewsForFlow,
  defaultFlowForView,
  defaultViewForFlow,
  defaultViewForMode,
  flowCompatibleWithView,
  hashForMode,
  modeForHash,
  modeForView,
  viewBelongsToMode,
  viewTypesForMode
} from "../docs/architext/src/presentation/viewSelection.js";

const views = [
  { id: "system", type: "system-map" },
  { id: "workflow", type: "workflow" },
  { id: "sequence", type: "sequence" },
  { id: "context", type: "c4-context" },
  { id: "deployment", type: "deployment" },
  { id: "risk", type: "risk-overlay" }
];

test("view selection maps persisted view types to top-level modes", () => {
  assert.equal(modeForView(undefined), "flows");
  assert.equal(modeForView({ type: "system-map" }), "flows");
  assert.equal(modeForView({ type: "workflow" }), "flows");
  assert.equal(modeForView({ type: "sequence" }), "sequence");
  assert.equal(modeForView({ type: "c4-container" }), "c4");
  assert.equal(modeForView({ type: "deployment" }), "deployment");
  assert.equal(modeForView({ type: "risk-overlay" }), "data-risks");
});

test("view selection chooses the first compatible view for a mode", () => {
  assert.equal(defaultViewForMode("sequence", views, views[0]).id, "sequence");
  assert.equal(defaultViewForMode("c4", views, views[0]).id, "context");
  assert.equal(defaultViewForMode("data-risks", views, views[0]).id, "risk");
  assert.equal(defaultViewForMode("release-truth", views, views[0]).id, "system");
  assert.equal(defaultViewForMode("rules", views, views[0]).id, "system");
});

test("view selection exposes workflow through the shared Flows projection policy", () => {
  assert.deepEqual(viewTypesForMode("flows"), ["system-map", "flow-explorer", "workflow", "dataflow"]);
});

test("view selection rejects stale active views after mode changes", () => {
  assert.equal(viewBelongsToMode({ type: "deployment" }, "deployment"), true);
  assert.equal(viewBelongsToMode({ type: "workflow" }, "flows"), true);
  assert.equal(viewBelongsToMode({ type: "deployment" }, "flows"), false);
  assert.equal(viewBelongsToMode({ type: "deployment" }, "release-truth"), true);
  assert.equal(viewBelongsToMode({ type: "deployment" }, "rules"), true);
  assert.equal(viewBelongsToMode(undefined, "flows"), false);
});

test("view selection supports direct hash links for top-level modes", () => {
  assert.equal(modeForHash("#releasetruth"), "release-truth");
  assert.equal(modeForHash("#release-truth"), "release-truth");
  assert.equal(modeForHash("#rules"), "rules");
  assert.equal(modeForHash("#datarisks"), "data-risks");
  assert.equal(hashForMode("release-truth"), "#releasetruth");
  assert.equal(hashForMode("rules"), "#rules");
});

const flowViews = [
  {
    id: "all",
    type: "system-map",
    lanes: [{ nodeIds: ["actor", "api", "db", "queue"] }]
  },
  {
    id: "runtime",
    type: "workflow",
    lanes: [{ nodeIds: ["actor", "api", "db"] }]
  },
  {
    id: "data-only",
    type: "dataflow",
    lanes: [{ nodeIds: ["api", "db"] }]
  },
  {
    id: "sequence",
    type: "sequence",
    lanes: [{ nodeIds: ["actor", "api", "db", "queue"] }]
  }
];

const selectableFlows = [
  {
    id: "request",
    steps: [
      { from: "actor", to: "api" },
      { from: "api", to: "db" }
    ]
  },
  {
    id: "async",
    steps: [
      { from: "api", to: "queue" }
    ]
  }
];

test("flow view compatibility requires every selected flow endpoint", () => {
  assert.equal(flowCompatibleWithView(selectableFlows[0], flowViews[1]), true);
  assert.equal(flowCompatibleWithView(selectableFlows[1], flowViews[1]), false);
});

test("flow projection choices are filtered to compatible flow/view pairs", () => {
  assert.deepEqual(compatibleFlowViewsForFlow(flowViews, selectableFlows[1]).map((view) => view.id), ["all"]);
  assert.deepEqual(compatibleFlowsForView(selectableFlows, flowViews[1]).map((flow) => flow.id), ["request"]);
});

test("view and flow defaults repair incompatible selected pairs", () => {
  assert.equal(defaultViewForFlow("flows", flowViews[1], flowViews, selectableFlows[1], flowViews[0]).id, "all");
  assert.equal(defaultViewForFlow("sequence", flowViews[3], flowViews, selectableFlows[1], flowViews[0]).id, "sequence");
  assert.equal(defaultFlowForView(flowViews[1], selectableFlows[1], selectableFlows, selectableFlows[0]).id, "request");
});

test("flow selection prefers narrower authored projections over broad system maps", () => {
  assert.equal(defaultViewForFlow("flows", flowViews[0], flowViews, selectableFlows[0], flowViews[0]).id, "runtime");
});
