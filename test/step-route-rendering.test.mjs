import assert from "node:assert/strict";
import test from "node:test";
import {
  sequenceActivationSpans,
  sequenceReturnSourceStep,
  sequenceStepMessageKind,
  stepRouteClassName,
  stepRouteLabelClassName,
  stepRouteMarkerClassName
} from "../viewer/src/presentation/stepRouteModel.js";

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

test("sequence route model distinguishes return and outbound message semantics", () => {
  assert.equal(sequenceStepMessageKind({ action: "send request", summary: "", from: "a", to: "b" }, 10, 100), "request");
  assert.equal(sequenceStepMessageKind({ kind: "return", action: "done", summary: "", from: "b", to: "a" }, 100, 10), "return");
  assert.equal(sequenceStepMessageKind({ action: "return result", summary: "", from: "b", to: "a" }, 100, 10), "request");
  assert.equal(sequenceStepMessageKind({ kind: "async", action: "publish", summary: "", from: "a", to: "work-queue" }, 10, 100), "async");
  assert.equal(sequenceStepMessageKind({ kind: "persistence", action: "persist", summary: "", from: "a", to: "event-store" }, 10, 100), "persistence");
});

test("sequence route model pairs returns with explicit or reverse outbound messages", () => {
  const outbound = { id: "request", from: "client", to: "service" };
  const unrelated = { id: "other", from: "client", to: "store" };

  assert.equal(sequenceReturnSourceStep({ returnOf: "request", from: "service", to: "client" }, [outbound, unrelated]), outbound);
  assert.equal(sequenceReturnSourceStep({ from: "service", to: "client" }, [outbound, unrelated]), outbound);
  assert.equal(sequenceReturnSourceStep({ returnOf: "missing", from: "service", to: "client" }, [outbound]), null);
});

test("sequence route model derives activation bars from outbound and return pairs", () => {
  const spans = sequenceActivationSpans([
    { id: "request", from: "client", to: "service", action: "call", summary: "" },
    { id: "nested", from: "service", to: "store", action: "load", summary: "" },
    { id: "nested-return", from: "store", to: "service", kind: "return", returnOf: "nested", action: "return data", summary: "" },
    { id: "request-return", from: "service", to: "client", kind: "return", returnOf: "request", action: "return result", summary: "" }
  ], 50);

  assert.deepEqual(spans.map(({ id, participantId, startIndex, endIndex, depth }) => ({
    id,
    participantId,
    startIndex,
    endIndex,
    depth
  })), [
    { id: "activation-request", participantId: "service", startIndex: 0, endIndex: 3, depth: 0 },
    { id: "activation-nested", participantId: "store", startIndex: 1, endIndex: 2, depth: 0 }
  ]);
});
