import assert from "node:assert/strict";
import test from "node:test";
import { iconForNodeType, iconForStep, iconLabel } from "../docs/architext/src/presentation/diagramIconModel.js";

test("diagram icon model maps node taxonomy to semantic icons", () => {
  assert.equal(iconForNodeType("actor"), "actor");
  assert.equal(iconForNodeType("service"), "service");
  assert.equal(iconForNodeType("queue"), "queue");
  assert.equal(iconForNodeType("data-store"), "database");
  assert.equal(iconForNodeType("external-service"), "external");
  assert.equal(iconForNodeType("unknown"), "node");
});

test("diagram icon model maps flow step semantics, including decision start and stop", () => {
  assert.equal(iconForStep({ kind: "decision" }, 2, 5), "decision");
  assert.equal(iconForStep({ kind: "start" }, 2, 5), "start");
  assert.equal(iconForStep({ kind: "stop" }, 2, 5), "stop");
  assert.equal(iconForStep({ kind: "persistence" }, 2, 5), "database");
  assert.equal(iconForStep({}, 0, 5), "start");
  assert.equal(iconForStep({}, 4, 5), "stop");
  assert.equal(iconForStep({}, 2, 5), "process");
});

test("diagram icon labels are human-readable for accessible SVGs", () => {
  assert.equal(iconLabel("decision"), "Decision");
  assert.equal(iconLabel("start"), "Start");
  assert.equal(iconLabel("stop"), "Stop");
});
