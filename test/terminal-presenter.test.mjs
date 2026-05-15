import assert from "node:assert/strict";
import test from "node:test";
import { statusLines } from "../src/adapters/cli/terminal-presenter.mjs";

test("terminal status presenter formats repository health without collecting it", () => {
  const lines = statusLines({
    target: "/tmp/repo",
    installed: true,
    cliVersion: "1.2.3",
    copiedInstallDetected: false,
    gitignoreMissing: ["docs/architext/dist/"],
    trackedGenerated: [],
    c4: { issues: ["view: duplicate node membership"], remainingIssues: [] },
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
    "Doctor repairs: 1",
    "Doctor repairs available:",
    "- view: remove duplicate node membership",
    "Validation: passed",
    "Architext validation passed",
    "C4 issues:",
    "- view: duplicate node membership",
    "Instruction files:",
    "- AGENTS.md: current Architext section",
    "Root scripts:",
    "- architext: ok"
  ]);
});
