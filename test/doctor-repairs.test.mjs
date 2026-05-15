import assert from "node:assert/strict";
import test from "node:test";
import { doctorRepairCategories, doctorRepairsForStatus } from "../src/domain/lifecycle/doctor-repairs.mjs";

test("doctor repairs are derived from status findings without CLI or filesystem state", () => {
  const repairs = doctorRepairsForStatus({
    c4: {
      repairChanges: [
        "c4-container: remove 1 duplicate node membership entry (api)",
        "c4-context: split dense c4-context view into 2 scoped views"
      ]
    }
  });

  assert.deepEqual(repairs, [
    {
      id: "c4:c4-container: remove 1 duplicate node membership entry (api)",
      category: "c4",
      file: "docs/architext/data/views.json",
      summary: "c4-container: remove 1 duplicate node membership entry (api)"
    },
    {
      id: "c4:c4-context: split dense c4-context view into 2 scoped views",
      category: "c4",
      file: "docs/architext/data/views.json",
      summary: "c4-context: split dense c4-context view into 2 scoped views"
    }
  ]);
});

test("doctor repair categories are unique and preserve first-seen order", () => {
  assert.deepEqual(doctorRepairCategories([
    { category: "c4" },
    { category: "instructions" },
    { category: "c4" }
  ]), ["c4", "instructions"]);
});
