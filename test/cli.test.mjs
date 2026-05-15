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

function run(args, cwd = repoRoot) {
  return execFileSync(process.execPath, [cli, ...args], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"]
  });
}

function tempRepo() {
  return mkdtempSync(path.join(os.tmpdir(), "architext-test-"));
}

function cleanup(dir) {
  rmSync(dir, { recursive: true, force: true });
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
    assert.equal(status.cliVersion, "1.1.0");
    assert.equal(status.installed, true);
    assert.equal(status.copiedInstallDetected, false);
    assert.equal(status.rootScripts.architext.recommended, false);
  } finally {
    cleanup(target);
  }
});

test("--help documents path defaults and common commands", () => {
  const output = run(["--help"]);

  assert.match(output, /architext <command> \[path\]/);
  assert.match(output, /\[path\] is optional and defaults to the current directory/);
  assert.match(output, /architext serve/);
  assert.match(output, /architext sync \.\.\/roboticus --dry-run/);
  assert.match(output, /Do not copy or edit package-owned viewer/);
});
