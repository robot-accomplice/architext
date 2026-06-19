import assert from "node:assert/strict";
import test from "node:test";
import { schemaMigrationPlan } from "../src/domain/lifecycle/schema-migrations.mjs";

test("schema migration planning is empty when data is current", () => {
  const plan = schemaMigrationPlan({ currentVersion: "1.5.0", targetVersion: "1.5.0" });

  assert.equal(plan.upToDate, true);
  assert.deepEqual(plan.pending, []);
});

test("schema migration planning classifies additive migrations", () => {
  const plan = schemaMigrationPlan({ currentVersion: "1.3.0", targetVersion: "1.5.0" });

  assert.equal(plan.upToDate, false);
  assert.equal(plan.pending[0].kind, "additive");
  assert.equal(plan.pending[0].file, "docs/architext/data/manifest.json");
  assert.match(plan.pending[0].summary, /apply additive schema migration 1\.3\.0 -> 1\.5\.0/);
});

test("schema migration planning classifies breaking migrations", () => {
  const plan = schemaMigrationPlan({ currentVersion: "1.5.0", targetVersion: "2.0.0" });

  assert.equal(plan.pending[0].kind, "breaking");
  assert.match(plan.pending[0].summary, /apply breaking schema migration 1\.5\.0 -> 2\.0\.0/);
});

test("schema migration planning refuses newer target data", () => {
  const plan = schemaMigrationPlan({ currentVersion: "2.0.0", targetVersion: "1.5.0" });

  assert.equal(plan.pending[0].kind, "unsupported");
  assert.match(plan.pending[0].summary, /install a newer Architext CLI before migrating/);
});

test("schema migration planning rejects malformed equal versions", () => {
  const plan = schemaMigrationPlan({ currentVersion: "x", targetVersion: "x" });

  assert.equal(plan.upToDate, false);
  assert.equal(plan.pending[0].kind, "invalid");
  assert.match(plan.pending[0].summary, /schemaVersion must be semantic version/);
});

test("schema migration planning rejects malformed target versions", () => {
  const plan = schemaMigrationPlan({ currentVersion: "1.5.0", targetVersion: "next" });

  assert.equal(plan.upToDate, false);
  assert.equal(plan.pending[0].kind, "invalid");
  assert.match(plan.pending[0].summary, /CLI schema version next is invalid/);
});
