import assert from "node:assert/strict";
import test from "node:test";
import { planDiagram } from "../viewer/src/routing/planDiagram.js";
import { diagramLayoutFor } from "../viewer/src/presentation/diagramLayout.js";
import { pairInternalCrossings } from "../viewer/src/routing/routeDiagnostics.js";
import { spreadUnitSlots, crossingsBetween } from "../viewer/src/routing/routeEdges.js";
import { MIN_LEGIBLE_GAP, RECIPROCAL_PARALLEL_OFFSET } from "../viewer/src/routing/routeConstants.js";

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

test("LLM hub mounts each reciprocal pair as a centred, legible parallel bundle (model-inference)", () => {
  const plan = planModelInference();
  const llm = plan.nodeRects.get("llm-service");

  // WHY: capacity now derives from the arrowhead legibility gap (routeConstants
  // MIN_LEGIBLE_GAP_ARROWHEADS), so the hub no longer crams all four provider-pair mounts onto
  // the bottom face — it distributes the reciprocal pairs across faces. Each pair must still
  // render as a clean parallel bundle (legibly spaced, no self-cross) centred on its face. The
  // bottom face carries exactly one such pair.
  const mounts = faceMounts(plan, "llm-service", "bottom");
  assert.equal(mounts.length, 2, `expected one reciprocal pair on llm-service bottom, got ${mounts.length}: ${mounts}`);

  const gap = mounts[1] - mounts[0];
  assert.ok(
    gap >= MIN_LEGIBLE_GAP && gap <= RECIPROCAL_PARALLEL_OFFSET + 2,
    `the bottom pair should be a legible parallel bundle (~${RECIPROCAL_PARALLEL_OFFSET}px), got ${gap.toFixed(1)}px`
  );

  // The pair sits centred on the face, not shoved to one side.
  const mean = (mounts[0] + mounts[1]) / 2;
  const faceCenter = llm.x + llm.width / 2;
  assert.ok(Math.abs(mean - faceCenter) <= 4, `pair centre ${mean.toFixed(1)} should sit at face centre ${faceCenter.toFixed(1)}`);

  // The whole hub stays free of reciprocal self-crossings.
  const relationships = MODEL_INFERENCE_STEPS.map(([id, from, to], index) => ({
    id, from, to, relationshipType: "flow", displayIndex: index + 1
  }));
  assert.deepEqual(pairInternalCrossings(plan.routes, relationships), []);
});

test("SQLite faces centre their reciprocal-pair mounts as a unit (memory-lifecycle T2)", () => {
  const plan = planMemoryLifecycle();
  const sqlite = plan.nodeRects.get("sqlite-store");

  // WHY: the looser capacity keeps each reciprocal pair together on one SQLite face instead of
  // spilling a lone mount elsewhere. The left and right faces now each carry a pair; the pair's
  // CENTRE must sit on the face centre (not shoved to one side) and its two mounts stay legibly apart.
  for (const side of ["left", "right"]) {
    const mounts = faceMounts(plan, "sqlite-store", side);
    assert.equal(mounts.length, 2, `expected a reciprocal pair on sqlite-store ${side}, got ${mounts.length}: ${mounts}`);
    const faceCenter = sqlite.y + sqlite.height / 2;
    const mean = (mounts[0] + mounts[1]) / 2;
    assert.ok(Math.abs(mean - faceCenter) <= 2, `pair centre on sqlite-store ${side} = ${mean.toFixed(1)} should sit at face centre ${faceCenter.toFixed(1)}`);
    assert.ok(mounts[1] - mounts[0] >= MIN_LEGIBLE_GAP, `the ${side} pair must stay legibly apart, got ${(mounts[1] - mounts[0]).toFixed(1)}px`);
  }
});

test("UP<->Memory facing runs stay legibly separated, the ingest pair straight (memory-lifecycle T1)", () => {
  const plan = planMemoryLifecycle();
  const memory = plan.nodeRects.get("memory-system");

  // WHY: under the looser capacity all four UP<->Memory runs share Memory's left face. The accepted
  // tradeoff (see routeConstants MIN_LEGIBLE_GAP_ARROWHEADS) is that the request/context pair takes a
  // short detour while the ingest pair stays straight; the durable guarantee is that the four mounts
  // never pack below the legibility gap.
  const mounts = faceMounts(plan, "memory-system", "left");
  assert.equal(mounts.length, 4, `expected 4 facing runs on memory-system left, got ${mounts.length}: ${mounts}`);

  let minGap = Infinity;
  for (let i = 1; i < mounts.length; i += 1) minGap = Math.min(minGap, mounts[i] - mounts[i - 1]);
  assert.ok(minGap >= MIN_LEGIBLE_GAP, `adjacent facing-run mounts must stay legibly apart, got ${minGap.toFixed(1)}px between ${JSON.stringify(mounts)}`);

  // The ingest reciprocal pair stays a straight horizontal run (its two endpoints share a y).
  for (const id of ["ingest-turn", "ingest-confirmed"]) {
    const route = plan.routes.get(id);
    assert.equal(route.bends, 0, `${id} should remain a straight facing run, got ${route.bends} bend(s)`);
    assert.equal(
      route.points[0].y, route.points.at(-1).y,
      `${id} endpoints should share a y (straight): ${JSON.stringify([route.points[0], route.points.at(-1)])}`
    );
  }
});

// T4 (pair-aware ordering, pair-internal first). The model-inference cloud pair —
// route-cloud (llm.bottom -> external.top) and cloud-provider-result (external.top ->
// llm.bottom) — is a directly-facing reciprocal pair that should render as two parallel
// vertical lines. The mounts are misaligned ({1033,1045} on llm.bottom vs {1010,1022} on
// external.top), so each line jogs and the two jogs cross twice. The facing-distribution
// pass skips it (the runs are 5-point jogged, and llm.bottom is a mixed hub face). The
// straightening pass must rebuild the pair as parallel straight runs with no self-crossing.
test("model-inference reciprocal pairs do not cross themselves", () => {
  const relationships = MODEL_INFERENCE_STEPS.map(([id, from, to], index) => ({
    id, from, to, relationshipType: "flow", displayIndex: index + 1
  }));
  const plan = planModelInference();
  assert.deepEqual(pairInternalCrossings(plan.routes, relationships), []);
});

// Gutter-lane order (farthest target -> outermost lane). record-route (llm -> observability) is a
// LONE edge to the farthest node on llm's right face; the reciprocal route-local pair got spread
// onto outer lanes (1142/1154) while record-route kept the inner stub (1102), so its long descent
// sliced both of the pair's horizontal stubs. The farthest-target edge must take the OUTERMOST lane
// and the clearing mount so it brackets over the shorter pair instead of crossing it.
test("model-inference record-route brackets the farthest target without crossing its siblings", () => {
  const plan = planModelInference();
  const recordRoute = plan.routes.get("record-route");
  assert.equal(crossingsBetween(recordRoute, plan.routes.get("route-local")), 0, "record-route must not cross route-local");
  assert.equal(crossingsBetween(recordRoute, plan.routes.get("local-provider-result")), 0, "record-route must not cross local-provider-result");
});

// The skill-plugin-lifecycle flow (same view) mounts FIVE edges on skill-plugin-system's
// left face: two reciprocal pairs (install-item/result with the websocket plane, and
// use-skill/context with the unified pipeline) plus the lone persist-skill-state. Spacing
// three unit CENTRES evenly across the 54px face leaves only ~13.5px between centres, and a
// reciprocal pair is ~12px wide — so the two pairs' facing endpoints land ~1.5px apart and
// render on top of each other. Distribution must reserve each unit's width so the GAPS
// between unit edges stay legible.
const SKILL_PLUGIN_STEPS = [
  ["load-catalog", "skill-plugin-system", "sqlite-store"],
  ["catalog-state-returned", "sqlite-store", "skill-plugin-system"],
  ["install-item", "websocket-control-plane", "skill-plugin-system"],
  ["install-result-returned", "skill-plugin-system", "websocket-control-plane"],
  ["persist-skill-state", "skill-plugin-system", "sqlite-store"],
  ["skill-state-confirmed", "sqlite-store", "skill-plugin-system"],
  ["use-skill", "unified-pipeline", "skill-plugin-system"],
  ["skill-context-returned", "skill-plugin-system", "unified-pipeline"]
];

function planSkillPlugin() {
  const relationships = SKILL_PLUGIN_STEPS.map(([id, from, to], index) => ({
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

test("spreadUnitSlots reserves unit width so adjacent reciprocal pairs do not collide", () => {
  // Zero-width (lone) units fall back to evenly spaced centres — identical to endpointSpreadOffset,
  // so lone-only faces are unchanged.
  assert.deepEqual(spreadUnitSlots([0, 0, 0], 54), [-13.5, 0, 13.5]);
  // Three 12px-wide pairs (half-width 6) on a 54px face. Even centres would sit 13.5px apart,
  // leaving only 1.5px between the pairs' facing endpoints. Width-aware slots even the edge gaps.
  const slots = spreadUnitSlots([6, 6, 6], 54);
  const facingGap = (slots[1] - 6) - (slots[0] + 6); // pair2 upper edge minus pair1 lower edge
  assert.ok(facingGap >= 4, `adjacent reciprocal pairs must not collide, got ${facingGap}px`);
});

const MIN_LEGIBLE_MOUNT_GAP = 4;

test("skill-plugin left face keeps its reciprocal-pair mounts from overlapping", () => {
  const plan = planSkillPlugin();
  const mounts = faceMounts(plan, "skill-plugin-system", "left");
  assert.ok(mounts.length >= 4, `expected multiple left-face mounts, got ${mounts.length}`);
  let minGap = Infinity;
  for (let i = 1; i < mounts.length; i += 1) minGap = Math.min(minGap, mounts[i] - mounts[i - 1]);
  assert.ok(
    minGap >= MIN_LEGIBLE_MOUNT_GAP,
    `adjacent left-face mounts must stay legibly apart, got ${minGap}px between ${JSON.stringify(mounts)}`
  );
});
