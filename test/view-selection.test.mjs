import assert from "node:assert/strict";
import test from "node:test";
import { defaultViewForMode, modeForView, viewBelongsToMode } from "../docs/architext/src/presentation/viewSelection.js";

const views = [
  { id: "system", type: "system-map" },
  { id: "sequence", type: "sequence" },
  { id: "context", type: "c4-context" },
  { id: "deployment", type: "deployment" },
  { id: "risk", type: "risk-overlay" }
];

test("view selection maps persisted view types to top-level modes", () => {
  assert.equal(modeForView(undefined), "flows");
  assert.equal(modeForView({ type: "system-map" }), "flows");
  assert.equal(modeForView({ type: "sequence" }), "sequence");
  assert.equal(modeForView({ type: "c4-container" }), "c4");
  assert.equal(modeForView({ type: "deployment" }), "deployment");
  assert.equal(modeForView({ type: "risk-overlay" }), "data-risks");
});

test("view selection chooses the first compatible view for a mode", () => {
  assert.equal(defaultViewForMode("sequence", views, views[0]).id, "sequence");
  assert.equal(defaultViewForMode("c4", views, views[0]).id, "context");
  assert.equal(defaultViewForMode("data-risks", views, views[0]).id, "risk");
});

test("view selection rejects stale active views after mode changes", () => {
  assert.equal(viewBelongsToMode({ type: "deployment" }, "deployment"), true);
  assert.equal(viewBelongsToMode({ type: "deployment" }, "flows"), false);
  assert.equal(viewBelongsToMode(undefined, "flows"), false);
});
