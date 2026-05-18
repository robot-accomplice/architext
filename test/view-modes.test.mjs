import assert from "node:assert/strict";
import test from "node:test";
import { modeShowsOrderedFlow, modeUsesStructuralRelationships } from "../docs/architext/src/presentation/viewModes.js";

test("view modes expose ordered flow overlays only when step definitions are shown", () => {
  assert.equal(modeShowsOrderedFlow("flows"), true);
  assert.equal(modeShowsOrderedFlow("sequence"), true);
  assert.equal(modeShowsOrderedFlow("data-risks"), true);
  assert.equal(modeShowsOrderedFlow("deployment"), false);
  assert.equal(modeShowsOrderedFlow("c4"), false);
});

test("topology modes use structural relationships instead of partial active flow routes", () => {
  assert.equal(modeUsesStructuralRelationships("deployment"), true);
  assert.equal(modeUsesStructuralRelationships("c4"), true);
  assert.equal(modeUsesStructuralRelationships("flows"), false);
  assert.equal(modeUsesStructuralRelationships("data-risks"), false);
  assert.equal(modeUsesStructuralRelationships("sequence"), false);
});
