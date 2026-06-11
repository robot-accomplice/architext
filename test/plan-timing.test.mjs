import assert from "node:assert/strict";
import test from "node:test";
import { buildPlanTiming } from "../viewer/src/routing/usePlannedDiagram.js";

test("buildPlanTiming turns phase marks into a per-phase breakdown", () => {
  const timing = buildPlanTiming({
    startedAt: 1000,
    completedAt: 121000,
    marks: [
      { label: "Routing edges", at: 1200 },
      { label: "Separating parallel runs", at: 16200 },
      { label: "Aligning mount points", at: 116200 }
    ],
    lastProgress: { label: "Aligning mount points", done: 4, total: 4, routesConsidered: 712345 }
  });

  assert.equal(timing.totalMs, 120000);
  assert.deepEqual(timing.phases, [
    { label: "Routing edges", ms: 15000 },
    { label: "Separating parallel runs", ms: 100000 },
    { label: "Aligning mount points", ms: 4800 }
  ]);
  assert.equal(timing.routesConsidered, 712345);
});

test("buildPlanTiming tolerates missing marks and progress", () => {
  const timing = buildPlanTiming({ startedAt: 500, completedAt: 700, marks: [], lastProgress: null });
  assert.equal(timing.totalMs, 200);
  assert.deepEqual(timing.phases, []);
  assert.equal(timing.routesConsidered, 0);
});
