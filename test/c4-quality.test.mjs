import assert from "node:assert/strict";
import test from "node:test";
import { c4IssuesForView, repairC4Views } from "../src/domain/architecture-model/c4-quality.mjs";

test("C4 quality diagnosis reports duplicates and type drift without filesystem access", () => {
  const nodes = [
    ["system", { id: "system", type: "software-system", dependencies: ["api"] }],
    ["api", { id: "api", type: "service", dependencies: [] }],
    ["component", { id: "component", type: "module", dependencies: [] }]
  ];
  const nodeMap = new Map(nodes);
  const view = {
    id: "container-view",
    type: "c4-container",
    lanes: [
      { id: "runtime", nodeIds: ["system", "api"] },
      { id: "components", nodeIds: ["api", "component"] }
    ]
  };

  assert.deepEqual(c4IssuesForView(view, nodeMap), [
    "container-view: duplicate node membership: api",
    "container-view: component has module, which does not belong in c4-container"
  ]);
});

test("C4 quality repair removes duplicate node membership deterministically", () => {
  const nodes = [
    ["system", { id: "system", type: "software-system", dependencies: [] }],
    ["api", { id: "api", type: "service", dependencies: [] }]
  ];
  const nodeMap = new Map(nodes);
  const views = [{
    id: "container-view",
    name: "Container View",
    type: "c4-container",
    lanes: [
      { id: "runtime", nodeIds: ["system", "api"] },
      { id: "shared", nodeIds: ["api"] }
    ]
  }];

  const repaired = repairC4Views(views, nodeMap);

  assert.deepEqual(repaired.changes, ["container-view: remove 1 duplicate node membership entry (api)"]);
  assert.deepEqual(repaired.views[0].lanes.map((lane) => lane.nodeIds), [["system", "api"], []]);
});
