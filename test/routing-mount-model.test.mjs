import assert from "node:assert/strict";
import test from "node:test";
import { surfaceSpacingCost, mountAssignmentCost, applyOffsetWithMatch, optimizeMountAssignments, routeIntersections, doglegCount, intentMismatchCount, excessLength, reciprocalParallelMoves, buildReciprocalGutterBridge } from "../viewer/src/routing/routeMountModel.js";
import { crossingsBetween } from "../viewer/src/routing/routeEdges.js";
import { MIN_LEGIBLE_GAP, MOUNT_COST } from "../viewer/src/routing/routeConstants.js";

// A left/right surface of length 54 with 3 mounts: ideal slots at 54*[1,2,3]/4.
test("evenly-spread mounts cost less than crammed mounts", () => {
  const length = 54;
  const ideal = [1, 2, 3].map((i) => (i / 4) * length); // 13.5, 27, 40.5
  const crammed = [26, 27, 28];                          // ~1px gaps
  assert.ok(surfaceSpacingCost(ideal, length, 3) < surfaceSpacingCost(crammed, length, 3));
});

test("a gap below MIN_LEGIBLE_GAP is penalized as cramped", () => {
  const length = 54;
  const wide = [10, 27, 44];        // gaps 17 — all >= MIN_LEGIBLE_GAP
  const tight = [25, 27, 29];       // gaps 2 — below MIN_LEGIBLE_GAP
  assert.equal(surfaceSpacingCost(wide, length, 3) < surfaceSpacingCost(tight, length, 3), true);
  assert.ok(MIN_LEGIBLE_GAP > 2);
});

test("a route through a non-endpoint node body costs at least one tier-0 collision unit", () => {
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 40 }],
    ["b", { x: 400, y: 0, width: 40, height: 40 }],
    ["mid", { x: 180, y: 0, width: 40, height: 40 }]   // sits in the straight corridor a->b
  ]);
  const input = { nodeRects, visibleNodeIds: new Set(["a", "b", "mid"]), canvasWidth: 480, canvasHeight: 60 };
  const relationshipById = new Map([["e", { id: "e", from: "a", to: "b", relationshipType: "flow" }]]);
  const clean = new Map([["e", { style: "orthogonal", points: [{ x: 40, y: 20 }, { x: 100, y: 20 }, { x: 100, y: -40 }, { x: 380, y: -40 }, { x: 380, y: 20 }, { x: 400, y: 20 }], bends: 4, samples: [] }]]);
  const through = new Map([["e", { style: "orthogonal", points: [{ x: 40, y: 20 }, { x: 400, y: 20 }], bends: 4, samples: [] }]]); // straight line crosses "mid"; same bends so only collision differs
  const c = (r) => mountAssignmentCost(r, relationshipById, input);
  // The straight route plows through "mid" (a non-endpoint node) -> tier-0 collision;
  // the detour avoids every node body but adds some wire length. The colliding route
  // must bear the full tier-0 penalty and stay far costlier than the clean detour.
  assert.ok(c(through) >= MOUNT_COST.collision, `colliding route must bear tier-0 collision, got ${c(through)}`);
  assert.ok(c(through) > c(clean), `tier-0 collision must dominate the detour's extra length/bends`);
});

test("co-shifting a straight facing edge's partner keeps it straight", () => {
  // a.right at y=20 -> b.left at y=20, straight horizontal edge.
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 60 }],
    ["b", { x: 200, y: 0, width: 40, height: 60 }]
  ]);
  const input = { nodeRects };
  const routeById = new Map([["e", { points: [{ x: 40, y: 20 }, { x: 200, y: 20 }], bends: 0 }]]);
  const relationshipById = new Map([["e", { id: "e", from: "a", to: "b", relationshipType: "flow" }]]);
  // Move a.right mount down by +10 (to y=30); partner b.left must follow to stay straight.
  applyOffsetWithMatch(routeById, relationshipById, input, { id: "e", endpointIndex: 0, side: "right", rect: nodeRects.get("a") }, 10);
  const pts = routeById.get("e").points;
  assert.equal(pts[0].y, pts[pts.length - 1].y, "edge stays straight (both ends moved together)");
  assert.equal(pts[0].y, 30);
});

test("optimizeMountAssignments never increases total cost and is idempotent", () => {
  // Two edges share a.right (h=120): one to b (top), one to c (bottom). Their mounts
  // start ~4px apart (crammed). Re-spreading must not raise cost and must converge.
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 120 }],
    ["b", { x: 240, y: 0, width: 40, height: 120 }],
    ["c", { x: 240, y: 160, width: 40, height: 120 }]
  ]);
  const input = {
    nodeRects, visibleNodeIds: new Set(["a", "b", "c"]),
    canvasWidth: 320, canvasHeight: 320, relationships: [], style: "orthogonal"
  };
  const routeById = new Map([
    ["ab", { style: "orthogonal", points: [{ x: 40, y: 58 }, { x: 240, y: 20 }], bends: 1, samples: [] }],
    ["ac", { style: "orthogonal", points: [{ x: 40, y: 62 }, { x: 240, y: 220 }], bends: 1, samples: [] }]
  ]);
  const relationshipById = new Map([
    ["ab", { id: "ab", from: "a", to: "b", relationshipType: "flow", displayIndex: 1 }],
    ["ac", { id: "ac", from: "a", to: "c", relationshipType: "flow", displayIndex: 2 }]
  ]);
  input.relationships = [...relationshipById.values()];
  const before = mountAssignmentCost(routeById, relationshipById, input);
  // No rebuild callback in this synthetic fixture -> offsets only.
  optimizeMountAssignments(routeById, relationshipById, input, { buildRouteForSides: null });
  const after = mountAssignmentCost(routeById, relationshipById, input);
  assert.ok(after <= before, `cost rose: ${before} -> ${after}`);
  const snapshot = JSON.stringify([...routeById].map(([id, r]) => [id, r.points]));
  optimizeMountAssignments(routeById, relationshipById, input, { buildRouteForSides: null });
  assert.equal(JSON.stringify([...routeById].map(([id, r]) => [id, r.points])), snapshot, "optimizer must be idempotent");
});

test("excessLength charges only the detour over the direct node-gap, not base length", () => {
  const a = { x: 0, y: 0, width: 40, height: 40 };
  const b = { x: 400, y: 0, width: 40, height: 40 };   // same row, 360px horizontal gap
  // A monotonic route straight across the gap carries NO length cost — it is the
  // shortest possible path between the nodes.
  const straight = { points: [{ x: 40, y: 20 }, { x: 400, y: 20 }] };           // length 360 = gap
  assert.equal(excessLength(straight, a, b), 0, "the direct path has zero excess");
  // A route that detours up and back over the gap is charged its avoidable overshoot.
  const detour = { points: [{ x: 40, y: 20 }, { x: 40, y: -80 }, { x: 400, y: -80 }, { x: 400, y: 20 }] }; // 100+360+100 = 560
  assert.equal(excessLength(detour, a, b), 200, "only the 2x100 detour over the 360 gap is charged");
  // A NECESSARY long edge (far nodes, monotonic) still costs nothing for length:
  // base distance is fixed by the layout and is not a quality defect.
  const farB = { x: 800, y: 0, width: 40, height: 40 };                         // 760px gap
  const straightFar = { points: [{ x: 40, y: 20 }, { x: 800, y: 20 }] };        // length 760 = gap
  assert.equal(excessLength(straightFar, a, farB), 0, "a necessary long edge has no length cost");
});

test("a longer route costs more than a shorter one with the same bends and mounts (length term)", () => {
  // Both edges mount at the SAME points (40,120)->(400,120) and have the SAME bend
  // count (2); they differ only in backtrack depth, i.e. wire length. With identical
  // mounts the spacing term is equal, so only a length term can separate them.
  const nodeRects = new Map([
    ["a", { x: 0, y: 100, width: 40, height: 40 }],
    ["b", { x: 400, y: 100, width: 40, height: 40 }]
  ]);
  const input = { nodeRects, visibleNodeIds: new Set(["a", "b"]), canvasWidth: 480, canvasHeight: 200 };
  const relationshipById = new Map([["e", { id: "e", from: "a", to: "b", relationshipType: "flow" }]]);
  const shallow = new Map([["e", { style: "orthogonal", points: [{ x: 40, y: 120 }, { x: 40, y: 60 }, { x: 400, y: 60 }, { x: 400, y: 120 }], bends: 2, samples: [] }]]); // length 480
  const deep = new Map([["e", { style: "orthogonal", points: [{ x: 40, y: 120 }, { x: 40, y: 20 }, { x: 400, y: 20 }, { x: 400, y: 120 }], bends: 2, samples: [] }]]);    // length 560
  assert.ok(
    mountAssignmentCost(deep, relationshipById, input) > mountAssignmentCost(shallow, relationshipById, input),
    "the longer (deeper backtrack) route must cost strictly more"
  );
});

test("legible but off-ideal mount spacing is free; only sub-legible crowding costs", () => {
  // Two mounts at 15 and 45 on a length-100 surface: gaps to the corners and between
  // are 15/30/55 — all >= MIN_LEGIBLE_GAP, so perfectly legible — but far from the ideal
  // even-spread slots (33,67). Legibility, not aesthetic evenness, is the cost driver.
  assert.equal(surfaceSpacingCost([15, 45], 100, 2), 0, "legible-but-uneven spacing must be free");
  // A genuinely crammed pair (2px gap) still costs.
  assert.ok(surfaceSpacingCost([49, 51], 100, 2) > 0, "sub-legible crowding must cost");
});

test("routeIntersections counts T-junctions but not shared mounts", () => {
  // T-junction: A is horizontal y=20 over x 0..100; B is vertical x=50 from y=20 down to
  // y=80, so B's TOP endpoint (50,20) lands ON A's interior. A strict "X" test misses
  // this; every intersection must count.
  const A = { points: [{ x: 0, y: 20 }, { x: 100, y: 20 }] };
  const B = { points: [{ x: 50, y: 20 }, { x: 50, y: 80 }] };
  assert.equal(routeIntersections(A, B), 1, "a T-junction is an intersection");
  // A clean X still counts.
  const C = { points: [{ x: 50, y: 0 }, { x: 50, y: 40 }] };
  assert.equal(routeIntersections(A, C), 1, "an X crossing counts");
  // Shared mount: both edges terminate at (0,0) — a legitimate convergence, not a crossing.
  const D = { points: [{ x: 0, y: 0 }, { x: 60, y: 0 }] };
  const E = { points: [{ x: 0, y: 0 }, { x: 0, y: 60 }] };
  assert.equal(routeIntersections(D, E), 0, "a shared mount is not an intersection");
});

test("doglegCount counts segments that backtrack against the overall direction", () => {
  const fromRect = { x: 0, y: 0, width: 40, height: 40 };    // center (20,20)
  const toRect = { x: 200, y: 0, width: 40, height: 40 };    // center (220,20) -> overall +x
  const monotonic = { points: [{ x: 40, y: 20 }, { x: 200, y: 20 }] };
  assert.equal(doglegCount(monotonic, fromRect, toRect), 0, "a monotonic route has no doglegs");
  // Jogs right, then LEFT (backtrack vs +x), then right again -> one dogleg segment.
  const backtrack = { points: [{ x: 40, y: 20 }, { x: 120, y: 20 }, { x: 120, y: 60 }, { x: 80, y: 60 }, { x: 80, y: 100 }, { x: 200, y: 100 }] };
  assert.equal(doglegCount(backtrack, fromRect, toRect), 1, "the reversing segment is a dogleg");
});

test("intentMismatchCount penalizes mounting on the side facing away from the target", () => {
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 40 }],
    ["b", { x: 200, y: 0, width: 40, height: 40 }]
  ]);
  const input = { nodeRects };
  const rel = { id: "e", from: "a", to: "b" };
  const facing = { points: [{ x: 40, y: 20 }, { x: 200, y: 20 }] };   // a.right -> b.left, both facing
  assert.equal(intentMismatchCount(facing, rel, input), 0, "facing mounts are not a mismatch");
  const away = { points: [{ x: 0, y: 20 }, { x: 200, y: 20 }] };       // a.LEFT faces away from b (to the right)
  assert.equal(intentMismatchCount(away, rel, input), 1, "mounting away from the target is a mismatch");
});

test("reciprocalParallelMoves runs an overlapping return parallel to its request (cost-guarded)", () => {
  // request a->b and return b->a both straight at y=30 -> fully overlapping (a shared segment).
  // The per-node-pair stage co-moves the return onto a parallel lane so the two no longer
  // overlap, and only when the lexicographic objective improves. This is the case the per-edge
  // sweep cannot reach: mirroring the return is a JOINT decision with its request.
  const nodeRects = new Map([
    ["a", { x: 0, y: 0, width: 40, height: 60 }],
    ["b", { x: 200, y: 0, width: 40, height: 60 }]
  ]);
  const input = { nodeRects, visibleNodeIds: new Set(["a", "b"]), canvasWidth: 280, canvasHeight: 60, relationships: [], style: "orthogonal" };
  const routeById = new Map([
    ["ab", { style: "orthogonal", points: [{ x: 40, y: 30 }, { x: 200, y: 30 }], bends: 0, samples: [] }],
    ["ba", { style: "orthogonal", points: [{ x: 200, y: 30 }, { x: 40, y: 30 }], bends: 0, samples: [] }]
  ]);
  const relationshipById = new Map([
    ["ab", { id: "ab", from: "a", to: "b", relationshipType: "flow", displayIndex: 1 }],
    ["ba", { id: "ba", from: "b", to: "a", relationshipType: "flow", displayIndex: 2 }]
  ]);
  input.relationships = [...relationshipById.values()];
  const before = mountAssignmentCost(routeById, relationshipById, input);
  reciprocalParallelMoves(routeById, relationshipById, input);
  const after = mountAssignmentCost(routeById, relationshipById, input);
  assert.ok(after < before, `expected cost to drop after parallel separation: ${before} -> ${after}`);
  assert.ok(routeById.get("ba").points.every((p) => p.y !== 30), "return must leave the shared y=30 lane");
});

test("buildReciprocalGutterBridge builds two nested, non-crossing routes over the gutter", () => {
  // A (left) and B (right) in the same row; a reciprocal pair a->b / b->a.
  const nodeRects = new Map([
    ["a", { x: 0, y: 100, width: 40, height: 40 }],
    ["b", { x: 200, y: 100, width: 40, height: 40 }]
  ]);
  const input = { nodeRects, visibleNodeIds: new Set(["a", "b"]), canvasWidth: 280, canvasHeight: 200, relationships: [] };
  const requestRel = { id: "ab", from: "a", to: "b", relationshipType: "flow", displayIndex: 1 };
  const returnRel = { id: "ba", from: "b", to: "a", relationshipType: "flow", displayIndex: 2 };
  const tmpl = { style: "orthogonal", points: [{ x: 40, y: 120 }, { x: 200, y: 120 }], bends: 0, samples: [] };
  const bridge = buildReciprocalGutterBridge(requestRel, returnRel, tmpl, tmpl, input, "top");
  assert.ok(bridge, "bridge should be constructible for same-row facing nodes");
  // The two routes must not cross each other (nested, stacked lanes) — the property the
  // construction guarantees by arithmetic, with no grid search.
  assert.equal(crossingsBetween(bridge.request, bridge.return), 0, "request and return must not cross");
  for (const r of [bridge.request, bridge.return]) {
    assert.equal(r.points[0].y, 100, "starts on a node top edge");
    assert.equal(r.points.at(-1).y, 100, "ends on a node top edge");
  }
  // The return nests on the outer (higher, smaller-y) lane so it encloses the request.
  const laneOf = (r) => Math.min(...r.points.map((p) => p.y));
  assert.ok(laneOf(bridge.return) < laneOf(bridge.request), "return nests on the outer lane");
});
