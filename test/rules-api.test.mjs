import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { deleteRule, moveRule, moveRuleBefore, upsertRule } from "../src/domain/architecture-model/rules.mjs";
import { updateRulesRequest } from "../src/adapters/http/rules-api.mjs";
import { writeJson, readJson } from "../src/adapters/cli/runtime.mjs";

const protectedRule = {
  id: "protected",
  title: "Protected",
  summary: "Protected rule.",
  category: "project",
  criticality: "critical",
  order: 10,
  source: "maintainer",
  protection: { edit: true, delete: true }
};

const editableRule = {
  id: "editable",
  title: "Editable",
  summary: "Editable rule.",
  category: "project",
  criticality: "critical",
  order: 20,
  source: "agent",
  protection: { edit: false, delete: false }
};

test("rules domain enforces edit and delete protection", () => {
  const document = { rules: [protectedRule, editableRule] };

  assert.throws(() => upsertRule(document, { ...protectedRule, summary: "Changed." }), /edit protected/);
  assert.throws(() => deleteRule(document, "protected"), /delete protected/);
  assert.equal(upsertRule(document, { ...editableRule, summary: "Changed." }).rules[1].summary, "Changed.");
  assert.deepEqual(deleteRule(document, "editable").rules.map((rule) => rule.id), ["protected"]);
});

test("rules domain reorders only unprotected peers in the same criticality group", () => {
  const document = {
    rules: [
      protectedRule,
      editableRule,
      { ...editableRule, id: "next", title: "Next", order: 30 }
    ]
  };

  const moved = moveRule(document, "next", "up");
  assert.deepEqual(moved.rules.map((rule) => [rule.id, rule.order]), [
    ["protected", 10],
    ["editable", 30],
    ["next", 20]
  ]);
  assert.throws(() => moveRule(document, "protected", "down"), /protected from reordering/);
});

test("rules domain supports drag-style reordering before an unprotected peer", () => {
  const document = {
    rules: [
      protectedRule,
      editableRule,
      { ...editableRule, id: "middle", title: "Middle", order: 30 },
      { ...editableRule, id: "last", title: "Last", order: 40 }
    ]
  };

  const moved = moveRuleBefore(document, "last", "middle");
  assert.deepEqual(moved.rules.map((rule) => [rule.id, rule.order]), [
    ["protected", 10],
    ["editable", 20],
    ["middle", 40],
    ["last", 30]
  ]);
  assert.throws(() => moveRuleBefore(document, "editable", "protected"), /protected from reordering/);
});

test("rules API writes structured rule updates and validates target data", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-rules-api-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  try {
    await mkdir(targetDataDir, { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      files: { rules: "rules.json" }
    });
    await writeJson(path.join(targetDataDir, "rules.json"), { rules: [editableRule] });

    await updateRulesRequest({
      target,
      payload: { action: "update", rule: { ...editableRule, summary: "Updated." } },
      dataDir: () => targetDataDir,
      readJson,
      writeJson,
      validateTarget: async () => ({ ok: true, output: "valid" })
    });

    const written = JSON.parse(await readFile(path.join(targetDataDir, "rules.json"), "utf8"));
    assert.equal(written.rules[0].summary, "Updated.");
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("rules API serializes direct write requests through the target lock", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-rules-api-lock-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  try {
    await mkdir(targetDataDir, { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      files: { rules: "rules.json" }
    });
    await writeJson(path.join(targetDataDir, "rules.json"), { rules: [editableRule] });

    const lockedTargets = [];
    await updateRulesRequest({
      target,
      payload: { action: "update", rule: { ...editableRule, summary: "Locked update." } },
      dataDir: () => targetDataDir,
      readJson,
      writeJson,
      validateTarget: async () => ({ ok: true, output: "valid" }),
      withTargetWriteLock: async (lockedTarget, callback) => {
        lockedTargets.push(lockedTarget);
        return callback();
      }
    });

    assert.deepEqual(lockedTargets, [target]);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("rules API treats missing action as a legacy update request", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-rules-api-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  try {
    await mkdir(targetDataDir, { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      files: { rules: "rules.json" }
    });
    await writeJson(path.join(targetDataDir, "rules.json"), { rules: [editableRule] });

    await updateRulesRequest({
      target,
      payload: { rule: { ...editableRule, summary: "Legacy update." } },
      dataDir: () => targetDataDir,
      readJson,
      writeJson,
      validateTarget: async () => ({ ok: true, output: "valid" })
    });

    const written = JSON.parse(await readFile(path.join(targetDataDir, "rules.json"), "utf8"));
    assert.equal(written.rules[0].summary, "Legacy update.");
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("rules API rejects unknown actions before writing", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-rules-api-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  try {
    await mkdir(targetDataDir, { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      files: { rules: "rules.json" }
    });
    await writeJson(path.join(targetDataDir, "rules.json"), { rules: [editableRule] });

    await assert.rejects(
      updateRulesRequest({
        target,
        payload: { action: "replace-everything", rule: { ...editableRule, summary: "Should not write." } },
        dataDir: () => targetDataDir,
        readJson,
        writeJson,
        validateTarget: async () => ({ ok: true, output: "valid" })
      }),
      /Unknown rules action "replace-everything"/
    );

    const written = JSON.parse(await readFile(path.join(targetDataDir, "rules.json"), "utf8"));
    assert.equal(written.rules[0].summary, "Editable rule.");
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("rules API rollback restores the exact on-disk bytes present before the failed write", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-rules-api-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  const rulesPath = path.join(targetDataDir, "rules.json");
  try {
    await mkdir(targetDataDir, { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      files: { rules: "rules.json" }
    });

    // Write the original file with non-canonical formatting (4-space indent, no
    // trailing newline). It parses to a valid rules document, but re-serializing
    // the in-memory read via writeJson would NOT reproduce these bytes. A correct
    // byte-snapshot rollback restores the file verbatim; a rollback that re-writes
    // the stale in-memory read silently rewrites the file's formatting.
    const originalBytes = JSON.stringify({ rules: [editableRule] }, null, 4);
    await writeFile(rulesPath, originalBytes, "utf8");

    await assert.rejects(
      updateRulesRequest({
        target,
        payload: { action: "update", rule: { ...editableRule, summary: "Invalid candidate." } },
        dataDir: () => targetDataDir,
        readJson,
        writeJson,
        validateTarget: async () => ({ ok: false, output: "rules schema failed" })
      }),
      /Rules update did not validate/
    );

    const afterRollback = await readFile(rulesPath, "utf8");
    assert.equal(
      afterRollback,
      originalBytes,
      "rollback must restore the exact bytes the adapter overwrote, not a re-serialization of the in-memory read snapshot"
    );
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("rules API supports protected-aware drag reorder actions", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-rules-api-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  try {
    await mkdir(targetDataDir, { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      files: { rules: "rules.json" }
    });
    await writeJson(path.join(targetDataDir, "rules.json"), {
      rules: [
        editableRule,
        { ...editableRule, id: "second", title: "Second", order: 30 }
      ]
    });

    await updateRulesRequest({
      target,
      payload: { action: "move-before", id: "second", beforeId: "editable" },
      dataDir: () => targetDataDir,
      readJson,
      writeJson,
      validateTarget: async () => ({ ok: true, output: "valid" })
    });

    const written = JSON.parse(await readFile(path.join(targetDataDir, "rules.json"), "utf8"));
    assert.deepEqual(written.rules.map((rule) => [rule.id, rule.order]), [
      ["editable", 30],
      ["second", 20]
    ]);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("rules API restores the previous document when validation rejects the candidate", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-rules-api-"));
  const targetDataDir = path.join(target, "docs", "architext", "data");
  try {
    await mkdir(targetDataDir, { recursive: true });
    await writeJson(path.join(targetDataDir, "manifest.json"), {
      files: { rules: "rules.json" }
    });
    await writeJson(path.join(targetDataDir, "rules.json"), { rules: [editableRule] });

    await assert.rejects(
      updateRulesRequest({
        target,
        payload: { action: "update", rule: { ...editableRule, summary: "Invalid candidate." } },
        dataDir: () => targetDataDir,
        readJson,
        writeJson,
        validateTarget: async () => ({ ok: false, output: "rules schema failed" })
      }),
      /Rules update did not validate/
    );

    const written = JSON.parse(await readFile(path.join(targetDataDir, "rules.json"), "utf8"));
    assert.equal(written.rules[0].summary, "Editable rule.");
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});
