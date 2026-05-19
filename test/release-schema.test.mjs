import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import Ajv2020 from "../docs/architext/node_modules/ajv/dist/2020.js";
import addFormats from "../docs/architext/node_modules/ajv-formats/dist/index.js";

const repoRoot = path.resolve(import.meta.dirname, "..");
const schema = JSON.parse(readFileSync(path.join(repoRoot, "docs", "architext", "schema", "release-detail.schema.json"), "utf8"));

function validator() {
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(ajv);
  ajv.addSchema(schema);
  return ajv.getSchema(schema.$id);
}

function releaseDetailWithItem(item) {
  return {
    id: "v1-3-0",
    version: "1.3.0",
    name: "Release Planning",
    status: "planned",
    posture: "on-track",
    summary: "Plan one specific next release.",
    targetWindow: "next",
    lastUpdated: "2026-05-18T06:05:00.000Z",
    scope: {
      required: [item],
      planned: [],
      stretch: [],
      deferred: [],
      outOfScope: []
    },
    workstreams: [],
    blockers: [],
    milestones: [],
    dependencies: [],
    evidence: []
  };
}

test("release items may record roadmap or ad hoc origin metadata", () => {
  const validate = validator();
  const detail = releaseDetailWithItem({
    id: "release-planning-contract",
    title: "Release planning data contract",
    kind: "architecture",
    status: "planned",
    summary: "Define release planning item metadata.",
    source: "ad-hoc",
    dateAdded: "2026-05-18T06:05:00.000Z"
  });

  assert.equal(validate(detail), true, JSON.stringify(validate.errors));
});

test("release item source metadata is constrained", () => {
  const validate = validator();
  const detail = releaseDetailWithItem({
    id: "release-planning-contract",
    title: "Release planning data contract",
    kind: "architecture",
    status: "planned",
    summary: "Define release planning item metadata.",
    source: "chat",
    dateAdded: "not-a-date"
  });

  assert.equal(validate(detail), false);
  assert.deepEqual(validate.errors.map((error) => `${error.instancePath}: ${error.message}`), [
    "/scope/required/0/source: must be equal to one of the allowed values",
    "/scope/required/0/dateAdded: must match format \"date-time\""
  ]);
});
