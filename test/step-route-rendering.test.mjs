import assert from "node:assert/strict";
import test from "node:test";
import {
  stepRouteClassName,
  stepRouteLabelClassName,
  stepRouteMarkerClassName
} from "../docs/architext/src/presentation/stepRouteModel.js";

test("flow and sequence step routes share the same presentation model", () => {
  assert.equal(stepRouteClassName("flow"), "flow-step-route");
  assert.equal(stepRouteClassName("sequence"), "sequence-step-route");

  assert.equal(stepRouteMarkerClassName(), "route-step-marker");
  assert.equal(stepRouteMarkerClassName("selected"), "route-step-marker selected");
  assert.equal(stepRouteLabelClassName(), "route-step-label");
  assert.equal(stepRouteLabelClassName("selected"), "route-step-label selected");
});

test("step route presentation model fails loudly for unknown route kinds", () => {
  assert.throws(() => stepRouteClassName("deployment"), /Unknown step route kind "deployment"/);
});
