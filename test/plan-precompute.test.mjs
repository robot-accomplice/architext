import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, writeFile, rm, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { enumerateFlowPlanRequests, createPlanPrecomputeFarm, planKeyHash } from "../src/adapters/http/plan-precompute.mjs";
import { deserializePlan } from "../viewer/src/routing/planCodec.js";

async function writeFixture(root) {
  const data = path.join(root, "docs", "architext", "data");
  await mkdir(data, { recursive: true });
  await writeFile(path.join(data, "flows.json"), JSON.stringify({
    flows: [
      {
        id: "f1",
        steps: [
          { id: "s1", from: "a", to: "b", action: "go", summary: "", data: [] },
          { id: "s2", from: "b", to: "c", action: "next", summary: "", data: [] }
        ]
      },
      { id: "f2", steps: [{ id: "s3", from: "a", to: "zz-not-in-view", action: "x", summary: "", data: [] }] }
    ]
  }));
  await writeFile(path.join(data, "views.json"), JSON.stringify({
    views: [
      {
        id: "v1",
        type: "flow-explorer",
        name: "V1",
        lanes: [
          { id: "l0", name: "a", nodeIds: ["a"] },
          { id: "l1", name: "b", nodeIds: ["b", "c"] }
        ]
      },
      { id: "v2", type: "c4-context", name: "C4", lanes: [{ id: "l", name: "x", nodeIds: ["a", "b", "c"] }] }
    ]
  }));
  return data;
}

test("enumerateFlowPlanRequests pairs flows with compatible flows-mode views only", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "architext-precompute-"));
  try {
    const dataDir = await writeFixture(root);
    const requests = await enumerateFlowPlanRequests({ dataDir, layoutConfig: undefined });
    // f1 fits v1; f2's endpoint is missing from v1; v2 is not a flows-mode view.
    assert.equal(requests.length, 1);
    assert.equal(requests[0].flowId, "f1");
    assert.equal(requests[0].viewId, "v1");
    assert.match(requests[0].hash, /^[0-9a-f]{64}$/);
    assert.equal(requests[0].hash, planKeyHash(requests[0].key));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("farm precomputes plans retrievable by exact key hash, and re-keys on refresh", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "architext-precompute-"));
  try {
    const dataDir = await writeFixture(root);
    let layout;
    const farm = createPlanPrecomputeFarm({
      target: root,
      dataDirFn: () => dataDir,
      loadConfigFn: async () => ({ diagram: { layout } }),
      log: () => {}
    });
    const settled = async () => {
      for (let i = 0; i < 200; i += 1) {
        const stats = farm.stats();
        if (stats.pending === 0) return stats;
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      throw new Error("farm did not settle");
    };

    await farm.refresh();
    const first = await settled();
    assert.equal(first.plans, 1, "one plan precomputed");

    const [request] = await enumerateFlowPlanRequests({ dataDir, layoutConfig: undefined });
    const stored = farm.lookup(request.hash);
    assert.ok(stored, "plan retrievable under sha256(planInputKey)");
    const plan = deserializePlan(JSON.parse(stored));
    assert.equal(plan.routes.size, 2, "both steps routed");
    assert.ok(plan.nodeRects instanceof Map);

    // A config change re-keys everything: old hash must miss after refresh.
    layout = { laneWidth: 444 };
    await farm.refresh();
    await settled();
    assert.equal(farm.lookup(request.hash), undefined, "old-config hash no longer served");
    const [rekeyed] = await enumerateFlowPlanRequests({ dataDir, layoutConfig: layout });
    assert.ok(farm.lookup(rekeyed.hash), "new-config plan present");
    farm.dispose();
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
