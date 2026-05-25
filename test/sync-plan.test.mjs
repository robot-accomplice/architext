import assert from "node:assert/strict";
import test from "node:test";
import {
  persistedSyncChoices,
  shouldValidateSync,
  syncMetadataPatch,
  syncOperation,
  syncWritePlan
} from "../src/adapters/cli/sync-plan.mjs";

const baseChoices = {
  branch: "current",
  instructionFiles: [],
  manageGitignore: false,
  manageRootScripts: false,
  applyDoctorRepairs: true,
  proceedWithChanges: true,
  promptBeforeProceed: false
};

const baseOptions = {
  force: false,
  dryRun: false,
  skipValidate: false
};

test("sync operation classifies install before migrate before sync", () => {
  assert.equal(syncOperation({ installing: true, migrating: true }), "install");
  assert.equal(syncOperation({ installing: false, migrating: true }), "migrate");
  assert.equal(syncOperation({ installing: false, migrating: false }), "sync");
});

test("sync write plan derives no-op and write-causing choices", () => {
  assert.deepEqual(syncWritePlan({
    installing: false,
    migrating: false,
    doctorRepairAvailable: false,
    syncChoices: baseChoices,
    options: baseOptions
  }), {
    doctorRepairsSelected: false,
    shouldWrite: false,
    operation: "sync",
    operationLabel: "Operation: sync (current)"
  });

  assert.equal(syncWritePlan({
    installing: false,
    migrating: false,
    doctorRepairAvailable: true,
    syncChoices: { ...baseChoices, applyDoctorRepairs: true },
    options: baseOptions
  }).shouldWrite, true);
  assert.equal(syncWritePlan({
    installing: false,
    migrating: false,
    doctorRepairAvailable: false,
    syncChoices: { ...baseChoices, instructionFiles: ["AGENTS.md"] },
    options: baseOptions
  }).shouldWrite, true);
  assert.equal(syncWritePlan({
    installing: false,
    migrating: false,
    doctorRepairAvailable: false,
    syncChoices: baseChoices,
    options: { ...baseOptions, force: true }
  }).shouldWrite, true);
});

test("sync validation is skipped only for explicit skip or dry-run install", () => {
  assert.equal(shouldValidateSync({ options: baseOptions, installing: false }), true);
  assert.equal(shouldValidateSync({ options: { ...baseOptions, skipValidate: true }, installing: false }), false);
  assert.equal(shouldValidateSync({ options: { ...baseOptions, dryRun: true }, installing: true }), false);
  assert.equal(shouldValidateSync({ options: { ...baseOptions, dryRun: true }, installing: false }), true);
});

test("sync metadata patch preserves persisted choice and validation contract", () => {
  const syncChoices = {
    ...baseChoices,
    branch: "new",
    instructionFiles: ["AGENTS.md"],
    manageGitignore: true,
    manageRootScripts: false,
    promptBeforeProceed: true
  };

  assert.deepEqual(syncMetadataPatch({
    version: "1.4.8",
    installing: false,
    migrating: true,
    instructionFiles: ["AGENTS.md", "CLAUDE.md"],
    syncChoices,
    managedInstructions: ["AGENTS.md"],
    gitignoreManaged: true,
    rootScriptsManaged: false,
    validation: { ok: true },
    now: "2026-05-25T12:00:00.000Z"
  }), {
    source: "architext-cli",
    cliVersion: "1.4.8",
    operation: "migrate",
    dataPolicy: "preserved",
    copiedInstallMigrated: true,
    instructionFiles: { "AGENTS.md": true, "CLAUDE.md": false },
    managedInstructions: ["AGENTS.md"],
    gitignoreManaged: true,
    rootScriptsManaged: false,
    syncChoices: persistedSyncChoices(syncChoices),
    lastValidation: { ok: true, at: "2026-05-25T12:00:00.000Z" }
  });
});
