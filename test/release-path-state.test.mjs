import assert from "node:assert/strict";
import test from "node:test";
import { toggleCollapsedReleasePathMilestone } from "../docs/architext/src/presentation/releasePathState.js";

test("release path collapse state toggles one milestone without mutating the current set", () => {
  const current = new Set(["complete-core"]);
  const collapsed = toggleCollapsedReleasePathMilestone(current, "blockers-cleared");

  assert.deepEqual([...current], ["complete-core"]);
  assert.deepEqual([...collapsed].sort(), ["blockers-cleared", "complete-core"]);
});

test("release path collapse state expands a collapsed milestone", () => {
  const current = new Set(["complete-core", "blockers-cleared"]);
  const collapsed = toggleCollapsedReleasePathMilestone(current, "complete-core");

  assert.deepEqual([...current].sort(), ["blockers-cleared", "complete-core"]);
  assert.deepEqual([...collapsed], ["blockers-cleared"]);
});
