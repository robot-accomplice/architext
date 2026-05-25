import assert from "node:assert/strict";
import test from "node:test";
import { printStatus, statusLines } from "../src/adapters/cli/terminal-presenter.mjs";

test("terminal status presenter formats repository health without collecting it", () => {
  const lines = statusLines({
    target: "/tmp/repo",
    installed: true,
    cliVersion: "1.2.3",
    copiedInstallDetected: false,
    gitignoreMissing: ["docs/architext/dist/"],
    trackedGenerated: [],
    c4: { issues: ["view: duplicate node membership"], drilldownIssues: ["context: system has no c4-container drilldown view"], remainingIssues: [] },
    manifest: {
      schemaVersion: "0.1.0",
      expectedSchemaVersion: "1.4.0",
      repairChanges: ["apply breaking schema migration 0.1.0 -> 1.4.0: update manifest.schemaVersion"],
      migrationPlan: {
        pending: [{ summary: "apply breaking schema migration 0.1.0 -> 1.4.0: update manifest.schemaVersion" }]
      }
    },
    doctorRepairs: [{ summary: "view: remove duplicate node membership" }],
    validation: { ok: true, output: "Architext validation passed" },
    instructionStatus: {
      "AGENTS.md": { exists: true, hasArchitextSection: true, mentionsCopiedTemplate: false }
    },
    rootScripts: {
      architext: { present: true, recommended: true }
    }
  }, { verbose: true });

  assert.deepEqual(lines, [
    "Target: /tmp/repo",
    "Architext data: installed",
    "CLI: 1.2.3",
    "Copied install: no",
    "Gitignore: missing docs/architext/dist/",
    "Generated artifacts tracked: none",
    "C4 documents: 1 issue",
    "C4 drilldown: 1 gap",
    "Schema: 0.1.0 (expected 1.4.0)",
    "Schema migrations: 1 pending",
    "Doctor repairs: 1",
    "Doctor repairs available:",
    "- view: remove duplicate node membership",
    "Validation: passed",
    "Architext validation passed",
    "C4 issues:",
    "- view: duplicate node membership",
    "C4 drilldown gaps requiring architecture documentation:",
    "- context: system has no c4-container drilldown view",
    "Instruction files:",
    "- AGENTS.md: current Architext section",
    "Root scripts:",
    "- architext: ok"
  ]);
});

test("terminal status presenter writes to an injected logger", () => {
  const lines = [];
  printStatus({
    target: "/tmp/repo",
    installed: true,
    cliVersion: "1.2.3",
    copiedInstallDetected: false,
    gitignoreMissing: [],
    trackedGenerated: [],
    doctorRepairs: []
  }, { verbose: false }, { log: (line) => lines.push(line) });

  assert.deepEqual(lines.slice(0, 3), [
    "Target: /tmp/repo",
    "Architext data: installed",
    "CLI: 1.2.3"
  ]);
});
