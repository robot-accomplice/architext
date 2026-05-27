import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { cp, mkdir } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..");
const cli = path.join(repoRoot, "tools", "architext-adopt.mjs");
const template = path.join(repoRoot, "docs", "architext");
const packageVersion = JSON.parse(readFileSync(path.join(repoRoot, "package.json"), "utf8")).version;

function run(args, cwd = repoRoot) {
  return execFileSync(process.execPath, [cli, ...args], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"]
  });
}

function runWithInput(args, input, cwd = repoRoot) {
  return execFileSync(process.execPath, [cli, ...args], {
    cwd,
    encoding: "utf8",
    input,
    stdio: ["pipe", "pipe", "pipe"]
  });
}

function tempRepo() {
  return mkdtempSync(path.join(os.tmpdir(), "architext-test-"));
}

function cleanup(dir) {
  rmSync(dir, { recursive: true, force: true });
}

function writeJson(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

test("sync installs data-only Architext into a fresh repository", () => {
  const target = tempRepo();
  try {
    writeFileSync(path.join(target, "package.json"), "{\"scripts\":{\"test\":\"echo test\"}}\n");

    run(["sync", target, "--yes", "--branch", "none"]);

    assert.equal(existsSync(path.join(target, "docs", "architext", "data", "manifest.json")), true);
    assert.equal(existsSync(path.join(target, "docs", "architext", ".architext.json")), true);
    assert.equal(existsSync(path.join(target, "docs", "architext", "src")), false);
    assert.equal(existsSync(path.join(target, "docs", "architext", "schema")), false);
    assert.equal(existsSync(path.join(target, "docs", "architext", "package.json")), false);
    assert.equal(existsSync(path.join(target, "docs", "architext", "data", "rules.json")), true);

    const packageJson = JSON.parse(readFileSync(path.join(target, "package.json"), "utf8"));
    assert.equal(packageJson.scripts.architext, "architext serve .");
    assert.match(run(["validate", target]), /Architext validation passed/);
  } finally {
    cleanup(target);
  }
});

test("sync caps generated starter project slugs", async () => {
  const parent = tempRepo();
  const target = path.join(parent, "this-is-a-very-long-project-name-that-would-otherwise-produce-unwieldy-generated-architecture-identifiers");
  try {
    await mkdir(target, { recursive: true });
    writeFileSync(path.join(target, "package.json"), "{\"scripts\":{\"test\":\"echo test\"}}\n");

    run(["sync", target, "--yes", "--branch", "none"]);

    const manifest = JSON.parse(readFileSync(path.join(target, "docs", "architext", "data", "manifest.json"), "utf8"));
    assert.ok(manifest.project.id.length <= 64);
    assert.doesNotMatch(manifest.project.id, /-$/);
  } finally {
    cleanup(parent);
  }
});

test("sync migrates copied installs without rewriting architecture data", async () => {
  const target = tempRepo();
  try {
    await mkdir(path.join(target, "docs"), { recursive: true });
    await cp(template, path.join(target, "docs", "architext"), { recursive: true });
    const beforeManifest = readFileSync(path.join(target, "docs", "architext", "data", "manifest.json"), "utf8");
    writeFileSync(path.join(target, "AGENTS.md"), "Intro\n\n## Architext Architecture Documentation\n\nOld copied instructions. Run cd docs/architext && npm run validate and edit docs/architext/src.\n\n## Other\n\nKeep this.\n");

    const dryRun = run(["sync", target, "--dry-run", "--yes", "--branch", "none"]);
    assert.match(dryRun, /Would remove copied package-owned files/);
    assert.match(dryRun, /Validation: passed/);

    run(["sync", target, "--yes", "--branch", "none"]);

    assert.equal(readFileSync(path.join(target, "docs", "architext", "data", "manifest.json"), "utf8"), beforeManifest);
    assert.equal(existsSync(path.join(target, "docs", "architext", "src")), false);
    assert.equal(existsSync(path.join(target, "docs", "architext", "schema")), false);
    assert.equal(existsSync(path.join(target, "docs", "architext", "tools")), false);
    assert.deepEqual(readdirSync(path.join(target, "docs", "architext")).sort(), [".architext.json", "data"]);

    const instructions = readFileSync(path.join(target, "AGENTS.md"), "utf8");
    assert.match(instructions, /architext validate \[path\]/);
    assert.match(instructions, /## Other/);
    assert.doesNotMatch(instructions, /npm run validate|docs\/architext\/src/);
  } finally {
    cleanup(target);
  }
});

test("status --json is machine-readable for explicit target paths", () => {
  const target = tempRepo();
  try {
    run(["sync", target, "--yes", "--branch", "none"]);
    const status = JSON.parse(run(["status", target, "--json"]));

    assert.equal(status.target, target);
    assert.equal(status.cliVersion, packageVersion);
    assert.equal(status.installed, true);
    assert.equal(status.copiedInstallDetected, false);
    assert.equal(status.rootScripts.architext.recommended, false);
  } finally {
    cleanup(target);
  }
});

test("doctor and sync apply deterministic repair categories", () => {
  const target = tempRepo();
  try {
    run(["sync", target, "--yes", "--branch", "none"]);
    const viewsPath = path.join(target, "docs", "architext", "data", "views.json");
    const systemId = JSON.parse(readFileSync(path.join(target, "docs", "architext", "data", "manifest.json"), "utf8")).project.id + "-system";
    const views = JSON.parse(readFileSync(viewsPath, "utf8"));
    views.views.push({
      id: "c4-duplicate",
      name: "Duplicate C4",
      type: "c4-container",
      summary: "Fixture with duplicate node membership.",
      lanes: [
        { id: "first", name: "First", nodeIds: ["project-team", systemId] },
        { id: "second", name: "Second", nodeIds: [systemId] }
      ]
    });
    writeJson(viewsPath, views);

    const doctor = run(["doctor", target, "--dry-run"]);
    assert.match(doctor, /C4 documents: 1 issue/);
    assert.match(doctor, /Doctor repairs: 1/);
    assert.match(doctor, /Dry run: no doctor repairs applied/);

    const declined = runWithInput(["doctor", target], "n\n");
    assert.match(declined, /Apply deterministic doctor repairs/);
    assert.match(declined, /No doctor repairs applied/);

    const doctorApply = run(["doctor", target, "--yes"]);
    assert.match(doctorApply, /Applied doctor repairs/);
    assert.match(doctorApply, /c4-duplicate: remove 1 duplicate node membership entry/);
    let repaired = JSON.parse(readFileSync(viewsPath, "utf8")).views.find((view) => view.id === "c4-duplicate");
    assert.deepEqual(repaired.lanes.map((lane) => lane.nodeIds), [["project-team", systemId], []]);
    assert.match(run(["doctor", target]), /C4 documents: ok/);

    writeJson(viewsPath, views);

    const dryRun = run(["sync", target, "--dry-run", "--yes", "--branch", "none"]);
    assert.match(dryRun, /Doctor repairs available/);
    assert.match(dryRun, /Would apply doctor repairs/);
    assert.match(dryRun, /c4-duplicate: remove 1 duplicate node membership entry/);

    run(["sync", target, "--yes", "--branch", "none"]);
    repaired = JSON.parse(readFileSync(viewsPath, "utf8")).views.find((view) => view.id === "c4-duplicate");
    assert.deepEqual(repaired.lanes.map((lane) => lane.nodeIds), [["project-team", systemId], []]);
    assert.match(run(["doctor", target]), /C4 documents: ok/);
  } finally {
    cleanup(target);
  }
});

test("sync diagnoses and seeds missing Release Truth data for existing installs", () => {
  const target = tempRepo();
  try {
    run(["sync", target, "--yes", "--branch", "none"]);
    const manifestPath = path.join(target, "docs", "architext", "data", "manifest.json");
    const releaseDir = path.join(target, "docs", "architext", "data", "releases");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    delete manifest.files.releases;
    writeJson(manifestPath, manifest);
    rmSync(releaseDir, { recursive: true, force: true });

    const dryRun = run(["sync", target, "--dry-run", "--yes", "--branch", "none"]);
    assert.match(dryRun, /Release Truth: not configured/);
    assert.match(dryRun, /Would apply doctor repairs/);
    assert.match(dryRun, /add starter Release Truth data and manifest\.files\.releases/);
    assert.equal(existsSync(path.join(releaseDir, "index.json")), false);

    run(["sync", target, "--yes", "--branch", "none"]);
    const repairedManifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    assert.equal(repairedManifest.files.releases, "releases/index.json");
    assert.equal(existsSync(path.join(releaseDir, "index.json")), true);
    assert.match(run(["validate", target]), /Architext validation passed/);
  } finally {
    cleanup(target);
  }
});

test("doctor repairs stale Architext data schema versions", () => {
  const target = tempRepo();
  try {
    run(["sync", target, "--yes", "--branch", "none"]);
    const manifestPath = path.join(target, "docs", "architext", "data", "manifest.json");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    writeJson(manifestPath, { ...manifest, schemaVersion: "0.1.0" });

    const dryRun = run(["doctor", target, "--dry-run"]);
    assert.match(dryRun, /Schema: 0\.1\.0 \(expected 1\.5\.0\)/);
    assert.match(dryRun, /Schema migrations: 1 pending/);
    assert.match(dryRun, /apply breaking schema migration 0\.1\.0 -> 1\.5\.0: update manifest\.schemaVersion/);

    run(["doctor", target, "--yes"]);
    assert.equal(JSON.parse(readFileSync(manifestPath, "utf8")).schemaVersion, "1.5.0");
  } finally {
    cleanup(target);
  }
});

test("doctor and sync migrate model-specific instruction rules into Rules data", async () => {
  const target = tempRepo();
  try {
    run(["sync", target, "--yes", "--branch", "none"]);
    writeFileSync(path.join(target, "AGENTS.md"), [
      "- Always prefer systemic fixes over local patches.",
      "- Keep release truth current when release state changes.",
      ""
    ].join("\n"));
    await mkdir(path.join(target, ".cursor", "rules"), { recursive: true });
    writeFileSync(path.join(target, ".cursor", "rules", "project.mdc"), [
      "# Project rules",
      "- Validate Architext data before claiming the documentation is current.",
      ""
    ].join("\n"));

    const dryRun = run(["doctor", target, "--dry-run"]);
    assert.match(dryRun, /Instruction rule migration: 3 candidate rules/);
    assert.match(dryRun, /Candidate rule: Always prefer systemic fixes over local patches/);
    assert.match(dryRun, /Rewrite pointer: AGENTS\.md/);
    assert.match(dryRun, /Rewrite pointer: \.cursor\/rules\/project\.mdc/);
    assert.match(dryRun, /Dry run: no doctor repairs applied/);

    run(["doctor", target, "--yes"]);
    const rules = JSON.parse(readFileSync(path.join(target, "docs", "architext", "data", "rules.json"), "utf8")).rules;
    assert.equal(rules.some((rule) => rule.summary === "Always prefer systemic fixes over local patches."), true);
    assert.equal(rules.some((rule) => rule.summary === "Keep release truth current when release state changes."), true);
    assert.equal(rules.some((rule) => rule.summary === "Validate Architext data before claiming the documentation is current."), true);
    assert.match(readFileSync(path.join(target, "AGENTS.md"), "utf8"), /docs\/architext\/data\/rules\.json/);
    assert.doesNotMatch(readFileSync(path.join(target, "AGENTS.md"), "utf8"), /Always prefer systemic fixes over local patches/);
    assert.match(readFileSync(path.join(target, ".cursor", "rules", "project.mdc"), "utf8"), /Do not duplicate long-lived project rules/);

    writeFileSync(path.join(target, ".cursorrules"), "- Prefer deterministic CLI repairs over manual JSON rewrites.\n");
    const syncDryRun = run(["sync", target, "--dry-run", "--yes", "--branch", "none"]);
    assert.match(syncDryRun, /Doctor repairs available/);
    assert.match(syncDryRun, /Would apply doctor repairs/);
    assert.match(syncDryRun, /migrate instruction rule: Prefer deterministic CLI repairs over manual JSON/);
  } finally {
    cleanup(target);
  }
});

test("prompt includes Release Truth maintenance rules for agents", () => {
  const output = run(["prompt", ".", "--mode", "architecture-change"]);

  assert.match(output, /manifest\.schemaVersion as the Architext data schema contract version/);
  assert.match(output, /docs\/architext\/data\/rules\.json/);
  assert.match(output, /not the installed CLI\/package version/);
  assert.match(output, /Keep Release Truth data current when release scope, blockers, milestones, evidence, target dates, dependencies, or posture changes/);
  assert.match(output, /Treat Release Truth as reviewed release state, not a planning scratchpad/);
  assert.match(output, /refresh the generated release index from those facts/);
  assert.match(output, /Keep Release Path labels concise/);
  assert.match(output, /rationale, blocker explanation, evidence, dependencies, and next actions in detail data/);
  assert.match(output, /roadmap\.json for release planning source items/);
  assert.match(output, /source: "roadmap"/);
  assert.match(output, /source: "ad-hoc"/);
  assert.match(output, /Keep flow diagrams free of orphaned elements/);
  assert.match(output, /every rendered node, edge, marker, and label must be traceable/);
  assert.match(output, /Remove disconnected context, connect it with a labeled relationship, or split it into a separate view/);
  assert.match(output, /semantic iconography over UML\/code diagrams/);
  assert.match(output, /decision, start, stop, async, persistence, artifact, return, and process semantics with step.kind/);
  assert.match(output, /create at least two outgoing outcome steps from the decision node/);
  assert.match(output, /branch lines to share the decision step number/);
  assert.match(output, /For sequence diagrams, create explicit return paths/);
  assert.match(output, /mark returns with kind: "return" and returnOf/);
  assert.match(output, /use sequenceFrames for loops, retries, optional branches, and transaction or consistency blocks/);
  assert.match(output, /Build C4 drilldown chains with explicit scopeNodeId metadata/);
  assert.match(output, /leave actors and external dependencies without child views/);
});

test("prompt includes source extraction draft guidance", () => {
  const output = run(["prompt", ".", "--mode", "source-extraction"]);

  assert.match(output, /draft proposed Architext data changes/);
  assert.match(output, /Do not apply the draft silently/);
  assert.match(output, /reviewable draft of proposed JSON changes/);
  assert.match(output, /source paths and confidence notes/);
  assert.match(output, /Validation remains required/);
});

test("skill prints the packaged Architext skill content", () => {
  const cwd = tempRepo();
  try {
    const output = run(["skill"], cwd);
    const expected = readFileSync(path.join(repoRoot, "skills", "architext", "SKILL.md"), "utf8").trimEnd();

    assert.equal(output.trimEnd(), expected);
    assert.match(output, /^---\nname: architext/m);
    assert.match(output, /description: Use when architecture, flows, C4 views/);
    assert.match(output, /# Architext/);
    assert.match(output, /Keep flows ordered and traceable/);
    assert.match(output, /Sequence diagrams must make round trips explicit/);
  } finally {
    cleanup(cwd);
  }
});

test("managed agent instructions include Release Truth source-of-truth rules", () => {
  const target = tempRepo();
  try {
    run(["sync", target, "--yes", "--branch", "none", "--append-agents"]);

    const agents = readFileSync(path.join(target, "AGENTS.md"), "utf8");
    const claude = readFileSync(path.join(target, "CLAUDE.md"), "utf8");
    for (const instructions of [agents, claude]) {
      assert.match(instructions, /Release Truth is the reviewed release source of truth/);
      assert.match(instructions, /manifest\.json` records the Architext data schema version/);
      assert.match(instructions, /breaking schema\s+changes require a major semver release/);
      assert.match(instructions, /completed work,\s+deferrals, reprioritization, blockers, dependencies, and next actions belong in\s+the release detail file/);
      assert.match(instructions, /Keep Release Path labels concise/);
      assert.match(instructions, /roadmap\.json` as the\s+roadmap source/);
      assert.match(instructions, /source: "roadmap"/);
      assert.match(instructions, /source:\s+"ad-hoc"/);
      assert.match(instructions, /docs\/architext\/data\/rules\.json/);
      assert.match(instructions, /protection\.edit/);
      assert.match(instructions, /criticality` and `order/);
      assert.match(instructions, /Keep flow diagrams free of orphaned elements/);
      assert.match(instructions, /Every rendered node, edge, marker,\s+and label must be traceable/);
      assert.match(instructions, /Remove disconnected context, connect it with a labeled relationship, or split it\s+into a separate view/);
      assert.match(instructions, /semantic iconography over UML\/code diagrams/);
      assert.match(instructions, /decision, start,\s+stop, async, persistence, artifact, return, and process semantics with\s+`step\.kind`/);
      assert.match(instructions, /For decision branches, set `step\.outcome`/);
      assert.match(instructions, /branch lines should share the decision step number/);
      assert.match(instructions, /create explicit return\s+paths\s+for request\/response, command\/result, event\/acknowledgement, and\s+failure-return\s+interactions/);
      assert.match(instructions, /kind:\s+"return"/);
      assert.match(instructions, /`sequenceFrames` for loops, retries, optional branches, and transaction or\s+consistency blocks/);
      assert.match(instructions, /C4 drilldown/);
      assert.match(instructions, /scopeNodeId/);
      assert.match(instructions, /Do not represent unreviewed planning proposals as current Release Truth facts/);
      assert.match(instructions, /Do not claim the architecture documentation is current if validation fails or\s+was skipped/);
    }
  } finally {
    cleanup(target);
  }
});

test("sync can reuse saved interactive choices", () => {
  const target = tempRepo();
  try {
    writeFileSync(path.join(target, "package.json"), "{\"scripts\":{\"test\":\"echo test\"}}\n");

    run(["sync", target, "--quiet", "--branch", "none"]);
    const metadataPath = path.join(target, "docs", "architext", ".architext.json");
    const metadata = JSON.parse(readFileSync(metadataPath, "utf8"));
    metadata.syncChoices = {
      branch: "none",
      instructionFiles: ["CLAUDE.md"],
      manageGitignore: false,
      manageRootScripts: false,
      applyDoctorRepairs: true,
      proceedWithChanges: true
    };
    writeJson(metadataPath, metadata);

    const saved = JSON.parse(readFileSync(metadataPath, "utf8"));
    assert.deepEqual(metadata.syncChoices, {
      branch: "none",
      instructionFiles: ["CLAUDE.md"],
      manageGitignore: false,
      manageRootScripts: false,
      applyDoctorRepairs: true,
      proceedWithChanges: true
    });

    rmSync(path.join(target, "AGENTS.md"), { force: true });
    rmSync(path.join(target, "CLAUDE.md"), { force: true });
    const reused = runWithInput(["sync", target, "--force"], "y\n");

    assert.match(reused, /Reuse saved sync choices from the last run/);
    assert.doesNotMatch(reused, /Create\/update AGENTS\.md/);
    assert.deepEqual(saved.syncChoices.instructionFiles, ["CLAUDE.md"]);
    assert.equal(existsSync(path.join(target, "CLAUDE.md")), true);
    assert.equal(existsSync(path.join(target, "AGENTS.md")), false);
  } finally {
    cleanup(target);
  }
});

test("--prompt bypasses saved sync choices and asks again", () => {
  const target = tempRepo();
  try {
    writeFileSync(path.join(target, "package.json"), "{\"scripts\":{\"test\":\"echo test\"}}\n");

    run(["sync", target, "--quiet", "--branch", "none"]);
    const prompted = runWithInput(["sync", target, "--force", "--prompt", "--no-agents", "--no-gitignore", "--no-root-scripts"], "y\n");

    assert.doesNotMatch(prompted, /Reuse saved sync choices from the last run/);
    assert.match(prompted, /Proceed with selected Architext changes in this branch/);
    const metadata = JSON.parse(readFileSync(path.join(target, "docs", "architext", ".architext.json"), "utf8"));
    assert.deepEqual(metadata.syncChoices.instructionFiles, []);
    assert.equal(metadata.syncChoices.manageGitignore, false);
    assert.equal(metadata.syncChoices.manageRootScripts, false);
  } finally {
    cleanup(target);
  }
});

test("--quiet sync selects defaults without prompting", () => {
  const target = tempRepo();
  try {
    writeFileSync(path.join(target, "package.json"), "{\"scripts\":{\"test\":\"echo test\"}}\n");

    const output = run(["sync", target, "--quiet"]);

    assert.doesNotMatch(output, /Create\/update AGENTS\.md/);
    assert.doesNotMatch(output, /Reuse saved sync choices/);
    assert.equal(existsSync(path.join(target, "AGENTS.md")), true);
    assert.equal(existsSync(path.join(target, "CLAUDE.md")), true);
    assert.equal(readFileSync(path.join(target, ".gitignore"), "utf8").includes("docs/architext/dist/"), true);
    assert.equal(JSON.parse(readFileSync(path.join(target, "package.json"), "utf8")).scripts.architext, "architext serve .");

    const metadata = JSON.parse(readFileSync(path.join(target, "docs", "architext", ".architext.json"), "utf8"));
    assert.deepEqual(metadata.syncChoices.instructionFiles, ["AGENTS.md", "CLAUDE.md"]);
    assert.equal(metadata.syncChoices.manageGitignore, true);
    assert.equal(metadata.syncChoices.manageRootScripts, true);
  } finally {
    cleanup(target);
  }
});

test("--help documents path defaults and common commands", () => {
  const output = run(["--help"]);

  assert.match(output, /architext <command> \[path\]/);
  assert.match(output, /--quiet\s+Accept default sync prompts without interactive questions/);
  assert.match(output, /--prompt\s+Force sync prompts instead of offering saved answers/);
  assert.match(output, /--foreground\s+Run serve in the current terminal until interrupted/);
  assert.match(output, /--background\s+Run serve detached and return control after startup/);
  assert.match(output, /--open\s+Open the local viewer in the system browser/);
  assert.match(output, /--no-open\s+Do not open the system browser/);
  assert.match(output, /--host <host>\s+Serve bind host\. Defaults to 127\.0\.0\.1/);
  assert.match(output, /--port <port>\s+Preferred serve port\. Defaults to 4317; use 0 for an OS-assigned port/);
  assert.match(output, /--status\s+Show the recorded serve process/);
  assert.match(output, /--stop\s+Stop the recorded serve process/);
  assert.match(output, /\[path\] is optional and defaults to the current directory/);
  assert.match(output, /skill\s+Print the Architext SKILL\.md content for LLM skill creation/);
  assert.match(output, /version\s+Print the Architext package version/);
  assert.match(output, /architext serve/);
  assert.match(output, /architext serve --foreground/);
  assert.match(output, /architext serve --open/);
  assert.match(output, /architext serve --background/);
  assert.match(output, /architext serve --background --open/);
  assert.match(output, /architext serve --status/);
  assert.match(output, /architext serve --stop/);
  assert.match(output, /architext serve --host 127\.0\.0\.1 --port 4517/);
  assert.match(output, /architext sync \. --dry-run/);
  assert.match(output, /architext skill/);
  assert.match(output, /Target repos should commit only project-owned Architext state/);
  assert.match(output, /optional AGENTS\.md, CLAUDE\.md, Cursor rule, or \.cursorrules pointers/);
  assert.match(output, /Do not copy or edit package-owned viewer, schema, tool, package, Vite,/);
  assert.match(output, /TypeScript, public asset, README, or generated dependency files/);
  assert.match(output, /project rules into docs\/architext\/data\/rules\.json/);
});

test("version command and flag print the package version", () => {
  assert.equal(run(["version"]).trim(), packageVersion);
  assert.equal(run(["--version"]).trim(), packageVersion);
  assert.equal(run(["-v"]).trim(), packageVersion);
});
