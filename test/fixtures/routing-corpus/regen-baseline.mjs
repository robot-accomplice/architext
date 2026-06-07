// Regenerate baseline.json from the current router output on the corpus.
// Run this ONLY when you have intentionally changed routing and reviewed the new
// diagrams: `node test/fixtures/routing-corpus/regen-baseline.mjs`.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { computeCorpusMetrics } from "./metrics.mjs";

const DIR = dirname(fileURLToPath(import.meta.url));
const metrics = computeCorpusMetrics();
writeFileSync(join(DIR, "baseline.json"), JSON.stringify(metrics, null, 2) + "\n");
console.log(`Wrote baseline for ${Object.keys(metrics).length} flows.`);
