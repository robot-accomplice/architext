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

    const packageJson = JSON.parse(readFileSync(path.join(target, "package.json"), "utf8"));
    assert.equal(packageJson.scripts.architext, "architext serve .");
    assert.match(run(["validate", target]), /Architext validation passed/);
  } finally {
    cleanup(target);
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

test("prompt includes Release Truth maintenance rules for LLM agents", () => {
  const output = run(["prompt", ".", "--mode", "architecture-change"]);

  assert.match(output, /Keep Release Truth data current when release scope, blockers, milestones, evidence, target dates, dependencies, or posture changes/);
  assert.match(output, /Treat Release Truth as reviewed release state, not a planning scratchpad/);
  assert.match(output, /refresh the generated release index from those facts/);
  assert.match(output, /Keep Release Path labels concise/);
  assert.match(output, /rationale, blocker explanation, evidence, dependencies, and next actions in detail data/);
});

test("managed agent instructions include Release Truth source-of-truth rules", () => {
  const target = tempRepo();
  try {
    run(["sync", target, "--yes", "--branch", "none", "--append-agents"]);

    const agents = readFileSync(path.join(target, "AGENTS.md"), "utf8");
    const claude = readFileSync(path.join(target, "CLAUDE.md"), "utf8");
    for (const instructions of [agents, claude]) {
      assert.match(instructions, /Release Truth is the reviewed release source of truth/);
      assert.match(instructions, /completed work,\s+deferrals, reprioritization, blockers, dependencies, and next actions belong in\s+the release detail file/);
      assert.match(instructions, /Keep Release Path labels concise/);
      assert.match(instructions, /Release Planning is a later Architext 1\.3\.0 capability/);
      assert.match(instructions, /Do not claim the architecture documentation is current if validation fails or\s+was skipped/);
    }
  } finally {
    cleanup(target);
  }
});

test("--help documents path defaults and common commands", () => {
  const output = run(["--help"]);

  assert.match(output, /architext <command> \[path\]/);
  assert.match(output, /\[path\] is optional and defaults to the current directory/);
  assert.match(output, /version\s+Print the Architext package version/);
  assert.match(output, /architext serve/);
  assert.match(output, /architext sync \. --dry-run/);
  assert.match(output, /Do not copy or edit package-owned viewer/);
});

test("version command and flag print the package version", () => {
  assert.equal(run(["version"]).trim(), packageVersion);
  assert.equal(run(["--version"]).trim(), packageVersion);
  assert.equal(run(["-v"]).trim(), packageVersion);
});
