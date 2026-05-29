import assert from "node:assert/strict";
import test from "node:test";
import { c4DrilldownUnavailableReason, childC4ViewForNode } from "../viewer/src/presentation/c4Drilldown.js";

test("C4 drilldown uses explicit scope metadata for child views", () => {
  const views = [
    { id: "context", type: "c4-context" },
    { id: "container", type: "c4-container", scopeNodeId: "system" },
    { id: "component", type: "c4-component", scopeNodeId: "api" }
  ];

  assert.equal(childC4ViewForNode(views, views[0], "system")?.id, "container");
  assert.equal(childC4ViewForNode(views, views[1], "api")?.id, "component");
});

test("C4 drilldown preserves normal selection when no scoped child view exists", () => {
  const views = [
    { id: "container", type: "c4-container", scopeNodeId: "system" },
    { id: "component", type: "c4-component", scopeNodeId: "api" }
  ];

  assert.equal(childC4ViewForNode(views, views[0], "database"), null);
  assert.equal(childC4ViewForNode(views, views[1], "api"), null);
  assert.equal(childC4ViewForNode(views, { id: "flow", type: "system-map" }, "api"), null);
});

test("C4 drilldown includes component to code and explains unavailable drilldowns", () => {
  const views = [
    { id: "component", type: "c4-component", scopeNodeId: "api" },
    { id: "code", type: "c4-code", scopeNodeId: "router" }
  ];

  assert.equal(childC4ViewForNode(views, views[0], "router")?.id, "code");
  assert.match(
    c4DrilldownUnavailableReason(views[0], { id: "mail", name: "Mail service", type: "external-service" }),
    /external dependency/
  );
  assert.match(
    c4DrilldownUnavailableReason({ id: "context", type: "c4-context" }, { id: "operator", name: "Operator", type: "actor" }),
    /actor/
  );
});
