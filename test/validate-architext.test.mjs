import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..");
const validator = path.join(repoRoot, "viewer", "tools", "validate-architext.mjs");
const sourceDataDir = path.join(repoRoot, "docs", "architext", "data");
const schemaDir = path.join(repoRoot, "viewer", "schema");

function runValidator(dataDir) {
  return spawnSync(process.execPath, [
    validator,
    "--data-dir",
    dataDir,
    "--schema-dir",
    schemaDir
  ], {
    cwd: repoRoot,
    encoding: "utf8"
  });
}

async function readFixtureJson(dataDir, fileName) {
  return JSON.parse(await readFile(path.join(dataDir, fileName), "utf8"));
}

async function writeFixtureJson(dataDir, fileName, value) {
  await writeFile(path.join(dataDir, fileName), `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

test("validate-architext reports malformed JSON without a raw parser stack", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-validate-json-"));
  const dataDir = path.join(target, "data");
  try {
    await cp(sourceDataDir, dataDir, { recursive: true });
    await writeFile(path.join(dataDir, "nodes.json"), "{ invalid json\n", "utf8");

    const result = runValidator(dataDir);
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

    const result = runValidator(dataDir);
    const output = `${result.stdout}\n${result.stderr}`;

    assert.equal(result.status, 1);
    assert.match(output, /step .*\.returnOf references unknown id "missing-step"/);
    assert.match(output, /sequenceFrame loop\.stepIds references unknown id "missing-frame-step"/);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("validate-architext checks roadmap target release references", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "architext-validate-roadmap-release-"));
  const dataDir = path.join(target, "data");
  try {
    await cp(sourceDataDir, dataDir, { recursive: true });
    const roadmap = await readFixtureJson(dataDir, "roadmap.json");
    roadmap.items[0].targetReleaseId = "missing-release";
    await writeFixtureJson(dataDir, "roadmap.json", roadmap);

    const result = runValidator(dataDir);
    const output = `${result.stdout}\n${result.stderr}`;

    assert.equal(result.status, 1);
    assert.match(output, /roadmap item .*\.targetReleaseId references unknown id "missing-release"/);
  } finally {
    await rm(target, { recursive: true, force: true });
  }
});

test("validate-architext checks each core reference relationship", async () => {
  const cases = [
    ["manifest.defaultViewId", "manifest.json", (manifest) => { manifest.defaultViewId = "missing-view"; }, /manifest\.defaultViewId references unknown id "missing-view"/],
    ["node.dependencies", "nodes.json", (nodes) => { nodes.nodes[0].dependencies = ["missing-node"]; }, /node .*\.dependencies references unknown id "missing-node"/],
    ["node.dataHandled", "nodes.json", (nodes) => { nodes.nodes[0].dataHandled = ["missing-data"]; }, /node .*\.dataHandled references unknown id "missing-data"/],
    ["node.relatedFlows", "nodes.json", (nodes) => { nodes.nodes[0].relatedFlows = ["missing-flow"]; }, /node .*\.relatedFlows references unknown id "missing-flow"/],
    ["node.relatedDecisions", "nodes.json", (nodes) => { nodes.nodes[0].relatedDecisions = ["missing-decision"]; }, /node .*\.relatedDecisions references unknown id "missing-decision"/],
    ["node.knownRisks", "nodes.json", (nodes) => { nodes.nodes[0].knownRisks = ["missing-risk"]; }, /node .*\.knownRisks references unknown id "missing-risk"/],
    ["flow.actors", "flows.json", (flows) => { flows.flows[0].actors = ["missing-actor"]; }, /flow .*\.actors references unknown id "missing-actor"/],
    ["flow.step.from", "flows.json", (flows) => { flows.flows[0].steps[0].from = "missing-from"; }, /step .*\.from references unknown id "missing-from"/],
    ["flow.step.to", "flows.json", (flows) => { flows.flows[0].steps[0].to = "missing-to"; }, /step .*\.to references unknown id "missing-to"/],
    ["flow.step.data", "flows.json", (flows) => { flows.flows[0].steps[0].data = ["missing-step-data"]; }, /step .*\.data references unknown id "missing-step-data"/],
    ["view.lane.nodeIds", "views.json", (views) => { views.views[0].lanes[0].nodeIds = ["missing-view-node"]; }, /view .* lane .* references unknown id "missing-view-node"/],
    ["decision.relatedNodes", "decisions.json", (decisions) => { decisions.decisions[0].relatedNodes = ["missing-decision-node"]; }, /decision .*\.relatedNodes references unknown id "missing-decision-node"/],
    ["decision.relatedFlows", "decisions.json", (decisions) => { decisions.decisions[0].relatedFlows = ["missing-decision-flow"]; }, /decision .*\.relatedFlows references unknown id "missing-decision-flow"/],
    ["risk.relatedNodes", "risks.json", (risks) => { risks.risks[0].relatedNodes = ["missing-risk-node"]; }, /risk .*\.relatedNodes references unknown id "missing-risk-node"/],
    ["risk.relatedFlows", "risks.json", (risks) => { risks.risks[0].relatedFlows = ["missing-risk-flow"]; }, /risk .*\.relatedFlows references unknown id "missing-risk-flow"/]
  ];

  for (const [name, fileName, mutate, expected] of cases) {
    const target = await mkdtemp(path.join(tmpdir(), "architext-validate-reference-"));
    const dataDir = path.join(target, "data");
    try {
      await cp(sourceDataDir, dataDir, { recursive: true });
      const value = await readFixtureJson(dataDir, fileName);
      mutate(value);
      await writeFixtureJson(dataDir, fileName, value);

      const result = runValidator(dataDir);
      const output = `${result.stdout}\n${result.stderr}`;

      assert.equal(result.status, 1, `${name} should fail validation`);
      assert.match(output, expected, `${name} should report the unknown reference`);
    } finally {
      await rm(target, { recursive: true, force: true });
    }
  }
});

test("validate-architext rejects malformed model shapes without raw stacks", async () => {
  const cases = [
    ["nodes array missing", "nodes.json", (nodes) => { delete nodes.nodes; }],
    ["flow steps malformed", "flows.json", (flows) => { flows.flows[0].steps = [{ id: "bad-step" }]; }],
    ["view lanes malformed", "views.json", (views) => { views.views[0].lanes = [{ id: "bad-lane", name: "Bad lane", nodeIds: [42] }]; }],
    ["risk severity malformed", "risks.json", (risks) => { risks.risks[0].severity = "catastrophic"; }]
  ];

  for (const [name, fileName, mutate] of cases) {
    const target = await mkdtemp(path.join(tmpdir(), "architext-validate-fuzz-"));
    const dataDir = path.join(target, "data");
    try {
      await cp(sourceDataDir, dataDir, { recursive: true });
      const value = await readFixtureJson(dataDir, fileName);
      mutate(value);
      await writeFixtureJson(dataDir, fileName, value);

      const result = runValidator(dataDir);
      const output = `${result.stdout}\n${result.stderr}`;

      assert.equal(result.status, 1, `${name} should fail validation`);
      assert.match(output, /Architext validation failed/);
      assert.doesNotMatch(output, /\n\s+at |TypeError:|SyntaxError:/);
    } finally {
      await rm(target, { recursive: true, force: true });
    }
  }
});
