import { test } from "node:test";
import assert from "node:assert/strict";
import { colorForOwner, buildFlowColorMap, ownerLegend, C4_COLOR } from "../viewer/src/presentation/repoTreeColors.js";

const nodes = [
  { id: "web", type: "client", sourcePaths: ["src/client/web.ts"], relatedFlows: ["f1"] },
  { id: "admin", type: "client", sourcePaths: ["src/client/admin.ts"], relatedFlows: ["f2"] },
  { id: "store", type: "data-store", sourcePaths: ["src/db/store.ts"], relatedFlows: ["f1"] },
  { id: "unmapped", type: "service", sourcePaths: [] } // no sourcePaths -> not an owner
];
const flows = [{ id: "f1", name: "Login" }, { id: "f2", name: "Admin" }, { id: "f3", name: "Unused" }];

test("colorForOwner uses C4 type under c4 lens and flow color under flow lens", () => {
  const flowColors = buildFlowColorMap(flows);
  assert.equal(colorForOwner(nodes[0], "c4", flowColors), C4_COLOR.client);
  assert.equal(colorForOwner(nodes[2], "c4", flowColors), C4_COLOR["data-store"]);
  assert.equal(colorForOwner(nodes[0], "flow", flowColors), flowColors.get("f1"));
  assert.equal(colorForOwner(null, "c4", flowColors), null);
});

test("ownerLegend lists only types/flows that actually own files (c4 dedupes types)", () => {
  const c4 = ownerLegend(nodes, flows, "c4");
  assert.deepEqual(c4.map((e) => e.key).sort(), ["client", "data-store"]); // service has no sourcePaths
  assert.equal(c4.find((e) => e.key === "client").label, "Client");
});

test("ownerLegend (flow lens) lists only flows referenced by owners", () => {
  const legend = ownerLegend(nodes, flows, "flow");
  assert.deepEqual(legend.map((e) => e.key).sort(), ["f1", "f2"]); // f3 unused -> excluded
  assert.equal(legend.find((e) => e.key === "f1").label, "Login");
});
