import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..");
const validator = path.join(repoRoot, "docs", "architext", "tools", "validate-architext.mjs");
const sourceDataDir = path.join(repoRoot, "docs", "architext", "data");
const schemaDir = path.join(repoRoot, "docs", "architext", "schema");

test("validate-architext reports malformed JSON without a raw parser stack", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-validate-json-"));
  const dataDir = path.join(target, "data");
  try {
    await cp(sourceDataDir, dataDir, { recursive: true });
    await writeFile(path.join(dataDir, "nodes.json"), "{ invalid json\n", "utf8");

    const result = spawnSync(process.execPath, [
      validator,
      "--data-dir",
      dataDir,
      "--schema-dir",
      schemaDir
    ], {
      cwd: repoRoot,
      encoding: "utf8"
    });
    const output = `${result.stdout}\n${result.stderr}`;

    assert.equal(result.status, 1);
    assert.match(output, /Architext validation failed/);
    assert.match(output, /Invalid JSON in .*nodes\.json/);
    assert.doesNotMatch(output, /at JSON\.parse|SyntaxError:/);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("validate-architext checks sequence return and frame step references", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-validate-sequence-"));
  const dataDir = path.join(target, "data");
  try {
    await cp(sourceDataDir, dataDir, { recursive: true });
    const flowsPath = path.join(dataDir, "flows.json");
    const flows = JSON.parse(await readFile(flowsPath, "utf8"));
    flows.flows[0].steps[0].returnOf = "missing-step";
    flows.flows[0].sequenceFrames = [{ id: "loop", type: "loop", label: "invalid", stepIds: ["missing-frame-step"] }];
    await writeFile(flowsPath, `${JSON.stringify(flows, null, 2)}\n`, "utf8");

    const result = spawnSync(process.execPath, [
      validator,
      "--data-dir",
      dataDir,
      "--schema-dir",
      schemaDir
    ], {
      cwd: repoRoot,
      encoding: "utf8"
    });
    const output = `${result.stdout}\n${result.stderr}`;

    assert.equal(result.status, 1);
    assert.match(output, /step .*\.returnOf references unknown id "missing-step"/);
    assert.match(output, /sequenceFrame loop\.stepIds references unknown id "missing-frame-step"/);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});
