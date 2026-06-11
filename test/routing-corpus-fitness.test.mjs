// Routing fitness ratchet over the sanitized high-complexity corpus.
//
// WHY: toy fixtures don't reproduce the routing challenges real diagrams hit, so
// regressions slipped through (a coplanar-blind dogleg metric, brittle synthetic
// assertions) while the corpus that DID stress the router lived in another repo and was
// skipped in CI. This runs the production planner over the in-repo corpus and asserts
// every gated metric equals a frozen baseline — locking in current quality so no change
// can silently make a complex diagram worse. When you intentionally change routing and
// review the new diagrams, re-baseline:
//   node test/fixtures/routing-corpus/regen-baseline.mjs
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { computeCorpusMetricsAndPerf, GATED_METRICS, PERF_GATED_COUNTERS } from "./fixtures/routing-corpus/metrics.mjs";

const BASELINE = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), "fixtures/routing-corpus/baseline.json"), "utf8")
);
const PERF_BASELINE = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), "fixtures/routing-corpus/perf-baseline.json"), "utf8")
);

// One planning pass feeds both gates, so quality and performance always judge
// the same plans.
const CORPUS = computeCorpusMetricsAndPerf();

test("corpus routing metrics hold the frozen baseline (no silent regression)", () => {
  const current = CORPUS.metrics;

  // Every baselined flow is still present and planned.
  assert.deepEqual(
    Object.keys(current).sort(),
    Object.keys(BASELINE).sort(),
    "corpus flow set changed; re-run regen-baseline.mjs if intentional"
  );

  const drift = [];
  for (const flow of Object.keys(BASELINE)) {
    for (const metric of GATED_METRICS) {
      const was = BASELINE[flow][metric];
      const now = current[flow][metric];
      if (now !== was) {
        const direction = now > was ? "REGRESSED" : "improved";
        drift.push(`${flow}.${metric}: ${was} -> ${now} (${direction})`);
      }
    }
  }

  assert.equal(
    drift.length,
    0,
    `routing metrics drifted from baseline:\n  ${drift.join("\n  ")}\n` +
      "If this is an intentional, reviewed routing change, regenerate the baseline:\n" +
      "  node test/fixtures/routing-corpus/regen-baseline.mjs"
  );
});

// Performance ratchet (maintainer policy 2026-06-11): thresholds are baselined on
// achieved results and move only in the direction of improvement
// (regen-perf-baseline.mjs refuses upward movement).
//
// Tier 1 — deterministic work counters: exact for fixed inputs on any machine, so any
// increase is a real volume regression, never flake.
// Tier 2 — machine-normalized wall ratio (corpus time / calibration workload): machine
// speed cancels out; the 1.35x headroom absorbs cross-machine code-mix drift while
// still tripping on every gross slowdown. Sub-1.35x pure-speed regressions ride under
// this ceiling — tightening it needs a calibrated perf runner, not a smaller constant.
const WALL_RATIO_HEADROOM = 1.35;

test("corpus planning performance holds the ratchet (counters exact, wall ratio bounded)", () => {
  const perf = CORPUS.perf;

  const drift = [];
  for (const [flowId, baselineCounters] of Object.entries(PERF_BASELINE.flows)) {
    for (const key of PERF_GATED_COUNTERS) {
      const now = perf.flows[flowId]?.[key];
      if (now === undefined) {
        drift.push(`${flowId}.${key}: missing from current run`);
      } else if (now > baselineCounters[key]) {
        drift.push(`${flowId}.${key}: ${baselineCounters[key]} -> ${now} (REGRESSED)`);
      }
    }
  }
  assert.equal(
    drift.length,
    0,
    `planner work volume regressed beyond the perf baseline:\n  ${drift.join("\n  ")}\n` +
      "The perf bar only moves toward improvement. Fix the regression; if the work " +
      "legitimately decreased elsewhere and you want to tighten the bar, run:\n" +
      "  node test/fixtures/routing-corpus/regen-perf-baseline.mjs"
  );

  const ceiling = PERF_BASELINE.wallRatio * WALL_RATIO_HEADROOM;
  assert.ok(
    perf.wallRatio <= ceiling,
    `normalized planning time regressed: wallRatio ${perf.wallRatio} > ceiling ${ceiling.toFixed(1)} ` +
      `(baseline ${PERF_BASELINE.wallRatio} x ${WALL_RATIO_HEADROOM})`
  );
});
