import assert from "node:assert/strict";
import test from "node:test";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";
import { diagramLayoutFor } from "../viewer/src/presentation/diagramLayout.js";

// Faithful fixture for the roboticus `model-inference` flow inside the `agent-turn-flow`
// view — the exact case the maintainer's live review flagged for uneven mounts. The LLM
// service hub carries two reciprocal pairs on its BOTTOM face: the pair to/from the
// Unified Pipeline (which detours around the Memory node sitting between them) and the
// pair to/from the cloud providers directly below. The reciprocal-parallel pass used to
// pin each return edge beside its request and never re-spread the pair *centres*, so all
// four mounts bunched toward the left of the face. The distribution pass treats each
// reciprocal pair as one unit and spreads the unit centres evenly; lone mounts (e.g. the
// observability edge, the cloud return) land centred on their face.
const VIEW = {
  id: "agent-turn-flow",
  name: "Agent Turn Flow",
  type: "flow-explorer",
  lanes: [
    { id: "entry", name: "Entry", nodeIds: ["operator", "cli", "tui", "web-dashboard", "websocket-control-plane", "external-channel-adapters"] },
    { id: "factory", name: "Factory", nodeIds: ["unified-pipeline"] },
    { id: "context-and-tools", name: "Context and Tools", nodeIds: ["memory-system", "product-knowledge-service", "skill-plugin-system", "mcp-system", "sqlite-store"] },
    { id: "inference-and-output", name: "Inference and Output", nodeIds: ["llm-service", "external-model-providers", "local-model-provider", "observability-system"] }
  ]
};

const MODEL_INFERENCE_STEPS = [
  ["prepare-request", "unified-pipeline", "llm-service"],
  ["route-cloud", "llm-service", "external-model-providers"],
  ["cloud-provider-result", "external-model-providers", "llm-service"],
  ["route-local", "llm-service", "local-model-provider"],
  ["local-provider-result", "local-model-provider", "llm-service"],
  ["record-route", "llm-service", "observability-system"],
  ["return-inference-result", "llm-service", "unified-pipeline"]
];

function planModelInference() {
  const relationships = MODEL_INFERENCE_STEPS.map(([id, from, to], index) => ({
    id, from, to, relationshipType: "flow", displayIndex: index + 1
  }));
  const layout = diagramLayoutFor(VIEW, relationships.length);
  return planDiagram({
    view: VIEW,
    relationships,
    visibleNodeIds: new Set(VIEW.lanes.flatMap((lane) => lane.nodeIds)),
    style: "orthogonal",
    ...layout
  });
}

// The memory-lifecycle flow (same view) drives lone mounts onto SQLite's faces — the
// maintainer flagged SQLite's west mount as off-centre even though it is the only mount
// there (T2). recenterSingletonSideEndpoints centres it early, but relief/optimize then
// pull it off; the final distribution pass re-centres it.
const MEMORY_LIFECYCLE_STEPS = [
  ["request-memory", "unified-pipeline", "memory-system"],
  ["query-store", "memory-system", "sqlite-store"],
  ["memory-records-returned", "sqlite-store", "memory-system"],
  ["memory-context-returned", "memory-system", "unified-pipeline"],
  ["ingest-turn", "unified-pipeline", "memory-system"],
  ["ingest-confirmed", "memory-system", "unified-pipeline"],
  ["curate-repair", "memory-system", "sqlite-store"],
  ["curation-result", "sqlite-store", "memory-system"]
];

function planMemoryLifecycle() {
  const relationships = MEMORY_LIFECYCLE_STEPS.map(([id, from, to], index) => ({
    id, from, to, relationshipType: "flow", displayIndex: index + 1
  }));
  const layout = diagramLayoutFor(VIEW, relationships.length);
  return planDiagram({
    view: VIEW,
    relationships,
    visibleNodeIds: new Set(VIEW.lanes.flatMap((lane) => lane.nodeIds)),
    style: "orthogonal",
    ...layout
  });
}

function endpointSide(rect, p) {
  if (p.x === rect.x) return "left";
  if (p.x === rect.x + rect.width) return "right";
  if (p.y === rect.y) return "top";
  if (p.y === rect.y + rect.height) return "bottom";
  return "";
}

// Coordinates (along the face axis) where edges mount `nodeId` on `side`.
function faceMounts(plan, nodeId, side) {
  const rect = plan.nodeRects.get(nodeId);
  const axis = side === "left" || side === "right" ? "y" : "x";
  const coords = [];
  for (const [, route] of plan.routes) {
    for (const point of [route.points[0], route.points.at(-1)]) {
      if (endpointSide(rect, point) !== side) continue;
      // endpointSide only checks the perpendicular coordinate, so an endpoint on another
      // node that happens to share this face's y (or x) would be misattributed — bound the
      // along-face coordinate to this node's extent so only its own mounts are counted.
      if (axis === "x" && (point.x < rect.x - 0.5 || point.x > rect.x + rect.width + 0.5)) continue;
      if (axis === "y" && (point.y < rect.y - 0.5 || point.y > rect.y + rect.height + 0.5)) continue;
      coords.push(point[axis]);
    }
  }
  return coords.sort((a, b) => a - b);
}

// Group sorted mounts into reciprocal pairs (a pair's two parallel mounts sit within the
// ~12px parallel offset of each other) and return each group's centre.
function unitCenters(coords, gap = 14) {
  if (coords.length === 0) return [];
  const groups = [[coords[0]]];
  for (let i = 1; i < coords.length; i += 1) {
    if (coords[i] - coords[i - 1] <= gap) groups[groups.length - 1].push(coords[i]);
    else groups.push([coords[i]]);
  }
  return groups.map((g) => g.reduce((a, b) => a + b, 0) / g.length);
}

// Even distribution of N units on a face of length L puts unit centres at the same
// fractions the router's own endpointSpreadOffset uses: (i+1)/(N+1) of the face.
function idealUnitCenters(rect, side, unitCount) {
  const start = side === "left" || side === "right" ? rect.y : rect.x;
  const length = side === "left" || side === "right" ? rect.height : rect.width;
  return Array.from({ length: unitCount }, (_, i) => start + ((i + 1) / (unitCount + 1)) * length);
}

test("LLM bottom face spreads its two reciprocal-pair centres evenly (model-inference)", () => {
  const plan = planModelInference();
  const llm = plan.nodeRects.get("llm-service");
  const mounts = faceMounts(plan, "llm-service", "bottom");

  assert.equal(mounts.length, 4, `expected 4 mounts on llm-service bottom, got ${mounts.length}: ${mounts}`);
  const centers = unitCenters(mounts);
  assert.equal(centers.length, 2, `expected 2 reciprocal pairs on llm-service bottom, got ${centers.length}: ${mounts}`);

  const ideal = idealUnitCenters(llm, "bottom", 2);
  const tol = 8;
  centers.forEach((center, i) => {
    assert.ok(
      Math.abs(center - ideal[i]) <= tol,
      `pair centre ${i} = ${center.toFixed(1)} should be within ${tol}px of even slot ${ideal[i].toFixed(1)} (mounts: ${mounts.map((m) => m.toFixed(1))})`
    );
  });

  // The whole set should be symmetric about the face centre, not shoved to one side.
  const mean = mounts.reduce((a, b) => a + b, 0) / mounts.length;
  const faceCenter = llm.x + llm.width / 2;
  assert.ok(Math.abs(mean - faceCenter) <= tol, `mounts mean ${mean.toFixed(1)} should be within ${tol}px of face centre ${faceCenter.toFixed(1)}`);
});

test("a lone mount on a SQLite face is centred, not left off to one side (memory-lifecycle T2)", () => {
  const plan = planMemoryLifecycle();
  const sqlite = plan.nodeRects.get("sqlite-store");

  // Each of these faces carries exactly one terminating mount; a lone mount should sit at the
  // face centre rather than wherever the upstream passes happened to leave it.
  for (const side of ["left", "top"]) {
    const mounts = faceMounts(plan, "sqlite-store", side);
    assert.equal(mounts.length, 1, `expected a single lone mount on sqlite-store ${side}, got ${mounts.length}: ${mounts}`);
    const faceCenter = side === "left" || side === "right" ? sqlite.y + sqlite.height / 2 : sqlite.x + sqlite.width / 2;
    assert.ok(
      Math.abs(mounts[0] - faceCenter) <= 2,
      `lone mount on sqlite-store ${side} = ${mounts[0].toFixed(1)} should be centred at ${faceCenter.toFixed(1)}`
    );
  }
});
