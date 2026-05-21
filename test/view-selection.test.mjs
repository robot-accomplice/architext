import assert from "node:assert/strict";
import test from "node:test";
import { defaultViewForMode, hashForMode, modeForHash, modeForView, viewBelongsToMode, viewTypesForMode } from "../docs/architext/src/presentation/viewSelection.js";

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
