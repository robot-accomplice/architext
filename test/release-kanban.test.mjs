import assert from "node:assert/strict";
import test from "node:test";
import { releaseKanbanColumns } from "../docs/architext/src/presentation/releaseKanban.js";

test("release kanban projects release truth items without creating a second task model", () => {
  const columns = releaseKanbanColumns({
    scope: {
      required: [
        { id: "contract", status: "complete" },
        { id: "blocked-item", status: "planned" },
        { id: "ready-item", status: "planned" },
        { id: "waiting-item", status: "planned", dependsOn: ["blocked-item"] },
        { id: "active-item", status: "in-progress" }
      ],
      planned: [{ id: "later-item", status: "planned", dependsOn: ["contract"] }],
      stretch: [{ id: "stretch-item", status: "stretch" }],
      deferred: [{ id: "deferred-item", status: "deferred" }],
      outOfScope: []
    },
    blockers: [
      { status: "blocked", itemIds: ["contract", "blocked-item"] },
      { status: "complete", itemIds: ["active-item"] }
    ]
  });

  const idsByColumn = new Map(columns.map((column) => [column.id, column.items.map((item) => item.id)]));

  assert.deepEqual(columns.map((column) => column.id), ["planned", "ready", "in-progress", "blocked", "deferred", "complete"]);
  assert.deepEqual(idsByColumn.get("planned"), ["waiting-item", "stretch-item"]);
  assert.deepEqual(idsByColumn.get("ready"), ["ready-item", "later-item"]);
  assert.deepEqual(idsByColumn.get("in-progress"), ["active-item"]);
  assert.deepEqual(idsByColumn.get("blocked"), ["blocked-item"]);
  assert.deepEqual(idsByColumn.get("deferred"), ["deferred-item"]);
  assert.deepEqual(idsByColumn.get("complete"), ["contract"]);
});
