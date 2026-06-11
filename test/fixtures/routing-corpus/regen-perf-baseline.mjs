// Regenerate perf-baseline.json — ALLOWED ONLY IN THE DIRECTION OF IMPROVEMENT.
//
// Maintainer policy (2026-06-11): the performance gate is baselined on achieved
// results and only ever tightens. If the current numbers are worse than the
// stored baseline this tool refuses; fix the regression instead of moving the
// bar. (Deliberate resets delete the baseline file first — visible in review.)
//
// Wall-time is stored as a machine-normalized ratio (corpus / calibration
// workload). The ratio is noisy run-to-run, so the stored value is the MEDIAN
// of three measurements; each runs in its own child process because the
// in-process raw-route cache would otherwise turn passes 2-3 into a different
// (cache-hit) workload — discovered when an in-process repeat tripped the
// counter-determinism assertion.
//   node test/fixtures/routing-corpus/regen-perf-baseline.mjs
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { computeCorpusMetricsAndPerf, PERF_GATED_COUNTERS } from "./metrics.mjs";

const SELF = fileURLToPath(import.meta.url);
const DIR = dirname(SELF);
const BASELINE_PATH = join(DIR, "perf-baseline.json");

if (process.argv.includes("--measure")) {
  process.stdout.write(JSON.stringify(computeCorpusMetricsAndPerf().perf));
  process.exit(0);
}

const passes = [];
for (let i = 0; i < 3; i += 1) {
  passes.push(JSON.parse(execFileSync(process.execPath, [SELF, "--measure"], { encoding: "utf8" })));
}
for (const later of passes.slice(1)) {
  if (JSON.stringify(later.flows) !== JSON.stringify(passes[0].flows)) {
    console.error("REFUSED: work counters differed between cold passes — planner is nondeterministic; investigate before baselining.");
    process.exit(1);
  }
}
const ratios = passes.map((pass) => pass.wallRatio).sort((a, b) => a - b);
const perf = { ...passes[0], wallRatio: ratios[1] };

if (existsSync(BASELINE_PATH)) {
  const previous = JSON.parse(readFileSync(BASELINE_PATH, "utf8"));
  const regressions = [];
  for (const [flowId, counters] of Object.entries(previous.flows ?? {})) {
    for (const key of PERF_GATED_COUNTERS) {
      const now = perf.flows[flowId]?.[key];
      if (now !== undefined && now > counters[key]) {
        regressions.push(`${flowId}.${key}: ${counters[key]} -> ${now}`);
      }
    }
  }
  // 10% noise tolerance: the refusal is about real regressions, not run jitter.
  if (perf.wallRatio > previous.wallRatio * 1.10) {
    regressions.push(`wallRatio: ${previous.wallRatio} -> ${perf.wallRatio}`);
  }
  if (regressions.length > 0) {
    console.error(
      "REFUSED: the perf baseline only moves toward improvement, but these measures regressed:\n  " +
        regressions.join("\n  ") +
        "\nFix the regression; do not move the bar."
    );
    process.exit(1);
  }
}

writeFileSync(BASELINE_PATH, JSON.stringify(perf, null, 2) + "\n");
console.log(`Wrote perf baseline: ${Object.keys(perf.flows).length} flows, wallRatio ${perf.wallRatio} (median of ${ratios.join(", ")}).`);
