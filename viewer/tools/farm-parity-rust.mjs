// Byte-parity gate for the native plan-precompute farm (Phase 1C).
//
// Compares the JS oracle (enumerateFlowPlanRequests + planDiagram + serializePlan)
// against the Rust native farm (farm_dump binary: cargo run --bin farm_dump).
//
// For each (flowId, viewId) pair:
//   - Identical SET of (flowId, viewId)
//   - Identical `key` string
//   - Identical `hash` (sha256 of key)
//   - Identical `planJson` (serialised plan, byte-for-byte)
//
// Exits nonzero if any request is RED.
//
// Usage: node viewer/tools/farm-parity-rust.mjs
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");

const { enumerateFlowPlanRequests, planKeyHash } = await import(path.join(repoRoot, "src/adapters/http/plan-precompute.mjs"));
const { planDiagram } = await import(path.join(repoRoot, "viewer/src/routing/planDiagram.js"));
const { serializePlan } = await import(path.join(repoRoot, "viewer/src/routing/planCodec.js"));
const { diagramConfigGetPayload } = await import(path.join(repoRoot, "src/adapters/http/diagram-config-api.mjs"));

const dataDir = path.join(repoRoot, "docs/architext/data");

// --- JS oracle ---
const config = await diagramConfigGetPayload(repoRoot);
const jsRequests = await enumerateFlowPlanRequests({
  dataDir,
  layoutConfig: config?.diagram?.layout,
});

// Build JS oracle map: "${flowId}@${viewId}" → { key, hash, planJson }
const jsMap = new Map();
for (const req of jsRequests) {
  const planResult = planDiagram(req.planInput);
  const planJson = JSON.stringify(serializePlan(planResult));
  jsMap.set(`${req.flowId}@${req.viewId}`, { key: req.key, hash: req.hash, planJson });
}

// --- Rust native farm (farm_dump binary) ---
let rustNdjson;
try {
  rustNdjson = execFileSync(
    "cargo",
    ["run", "-q", "-p", "architext-routing", "--features", "native", "--bin", "farm_dump", "--", dataDir],
    { cwd: repoRoot, encoding: "utf8", timeout: 120000 }
  );
} catch (err) {
  console.error("farm_dump failed:", err.message);
  process.exitCode = 1;
  process.exit();
}

// Parse NDJSON
const rustMap = new Map();
for (const line of rustNdjson.split("\n").filter(Boolean)) {
  const entry = JSON.parse(line);
  rustMap.set(`${entry.flowId}@${entry.viewId}`, entry);
}

// --- Diff ---
const allKeys = new Set([...jsMap.keys(), ...rustMap.keys()]);
let green = 0;
const rows = [];

for (const id of [...allKeys].sort()) {
  const js = jsMap.get(id);
  const rust = rustMap.get(id);

  if (!js && rust) {
    rows.push({ id, status: "RED", note: "RUST_ONLY (missing from JS)" });
    continue;
  }
  if (js && !rust) {
    rows.push({ id, status: "RED", note: "JS_ONLY (missing from Rust)" });
    continue;
  }

  const keyOk = js.key === rust.key;
  const hashOk = js.hash === rust.hash;
  const planOk = js.planJson === rust.planJson;
  const ok = keyOk && hashOk && planOk;

  if (ok) {
    green += 1;
    rows.push({ id, status: "GREEN", note: "" });
  } else {
    const notes = [];
    if (!keyOk) notes.push(`key_mismatch(js=${js.key.length}b rust=${rust.key.length}b)`);
    if (!hashOk) notes.push(`hash_mismatch(js=${js.hash.slice(0,12)} rust=${rust.hash.slice(0,12)})`);
    if (!planOk) notes.push(`planJson_mismatch(js=${js.planJson.length}b rust=${rust.planJson.length}b)`);
    rows.push({ id, status: "RED", note: notes.join(" ") });
  }
}

const total = allKeys.size;
for (const r of rows) {
  const marker = r.status === "GREEN" ? "✓" : "✗";
  const note = r.note ? `  ${r.note}` : "";
  console.log(`${marker} ${r.status.padEnd(5)} ${r.id.padEnd(50)}${note}`);
}
console.log(`\n${green}/${total} GREEN`);
if (green !== total) process.exitCode = 1;
