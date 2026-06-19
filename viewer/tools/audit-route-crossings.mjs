// Route-crossing audit harness.
//
// The mount-audit only measures distribution evenness, so reciprocal pairs that cross
// themselves (pair-internal crossings) and gutter lanes ordered wrong (farthest target not
// outermost) went unflagged in the live review. This tool runs the committed route diagnostics
// (the SAME detectors the viewer overlay and the unit tests use — pair-internal-crossing and
// lane-order-violation) over every flow in a data directory, so the defect surface is visible
// before and after a routing change.
//
// Usage:
//   node viewer/tools/audit-route-crossings.mjs --data-dir <dir>   # defaults to docs/architext/data
//
// It reports per flow×view and a grand total; it is informational (exit 0) and does not gate.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { planDiagram } from "../src/routing/planDiagram.js";
import { diagramLayoutFor } from "../src/presentation/diagramLayout.js";
import { diagnosePlannedRoutes } from "../src/routing/routeDiagnostics.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");

function parseArgs(argv) {
  const options = { dataDir: path.join(repoRoot, "docs", "architext", "data"), flow: "" };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--data-dir") options.dataDir = path.resolve(argv[++index] ?? "");
    else if (arg === "--flow") options.flow = argv[++index] ?? "";
    else throw new Error(`Unknown argument: ${arg}`);
  }
  return options;
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

const options = parseArgs(process.argv.slice(2));
const views = readJson(path.join(options.dataDir, "views.json")).views ?? [];
const flows = readJson(path.join(options.dataDir, "flows.json")).flows ?? [];

let totalPairInternal = 0;
let totalLaneOrder = 0;
let flowsWithDefects = 0;
let flowsScanned = 0;

for (const view of views) {
  const viewNodeIds = new Set((view.lanes ?? []).flatMap((lane) => lane.nodeIds ?? []));
  if (viewNodeIds.size === 0) continue;
  const eligible = flows.filter(
    (flow) => (flow.steps ?? []).length && flow.steps.every((step) => viewNodeIds.has(step.from) && viewNodeIds.has(step.to))
  );
  for (const flow of eligible) {
    if (options.flow && flow.id !== options.flow) continue;
    flowsScanned += 1;
    const relationships = flow.steps.map((step, index) => ({
      id: step.id,
      from: step.from,
      to: step.to,
      relationshipType: "flow",
      displayIndex: index + 1
    }));
    const layout = diagramLayoutFor(view, relationships.length);
    const plan = planDiagram({ view, relationships, visibleNodeIds: viewNodeIds, style: "orthogonal", ...layout });
    const diagnostics = plan.diagnostics ?? diagnosePlannedRoutes(plan, relationships);
    const pairInternal = diagnostics.findings.filter((finding) => finding.code === "pair-internal-crossing");
    const laneOrder = diagnostics.findings.filter((finding) => finding.code === "lane-order-violation");
    if (pairInternal.length === 0 && laneOrder.length === 0) continue;
    flowsWithDefects += 1;
    totalPairInternal += pairInternal.reduce((sum, finding) => sum + (finding.crossings ?? 1), 0);
    totalLaneOrder += laneOrder.length;
    console.log(`\n## [${view.id}] ${flow.id}`);
    for (const finding of pairInternal) {
      console.log(`   ⛔ pair-internal-crossing: ${finding.relationshipId} × ${finding.pairWith} = ${finding.crossings}`);
    }
    for (const finding of laneOrder) {
      console.log(`   ↔ lane-order: ${finding.nodeId}.${finding.side} farthest=${finding.farthest} not outermost=${finding.outermost}`);
    }
  }
}

console.log(
  `\n=== flows scanned ${flowsScanned} | with defects ${flowsWithDefects} | pair-internal crossings ${totalPairInternal} | lane-order violations ${totalLaneOrder} ===`
);
