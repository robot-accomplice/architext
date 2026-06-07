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
import { computeCorpusMetrics, GATED_METRICS } from "./fixtures/routing-corpus/metrics.mjs";

const BASELINE = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), "fixtures/routing-corpus/baseline.json"), "utf8")
);

test("corpus routing metrics hold the frozen baseline (no silent regression)", () => {
  const current = computeCorpusMetrics();

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
