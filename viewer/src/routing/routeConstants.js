export const CANVAS_INSET = {
  left: 24,
  right: 24,
  top: 30,
  bottom: 24
};

export const ROUTE_COST_WEIGHTS = {
  pointCount: 24,
  bend: 420,
  dogleg: 14000,
  sideDirection: 260,
  perimeterFallback: 7000,
  cornerPerimeterFallback: 12000,
  perimeterLength: 8,
  cornerPerimeterLength: 10,
  directPortReward: -2000,
  straightReward: -2200,
  splineReward: -1400,
  splineFlatPenalty: 90000,
  boundaryViolation: 14000,
  nodeCollision: 12000,
  nodeClearance: 120,
  monotonicBacktrack: 18,
  fixedPreferredGutter: 36
};

export const ROUTE_SPACING = {
  pairOffset: 40,
  indexOffsetModulo: 6,
  indexOffset: 14,
  splinePairOffset: 8,
  splineSpreadModulo: 7,
  splineSpread: 10,
  splineMinCurve: 36,
  splineMaxCurve: 180
};

export const SPLINE_CURVE_VARIANTS = [
  { multiplier: 1, spread: 1 },
  { multiplier: -1, spread: -1 },
  { multiplier: 0.72, spread: 0 },
  { multiplier: -0.72, spread: 0 },
  { multiplier: 1.36, spread: 1 },
  { multiplier: -1.36, spread: -1 },
  { multiplier: 2.1, spread: 1 },
  { multiplier: -2.1, spread: -1 },
  { multiplier: 0.38, spread: 0 },
  { multiplier: -0.38, spread: 0 }
];

export function rectCenter(rect) {
  return {
    x: rect.x + rect.width / 2,
    y: rect.y + rect.height / 2
  };
}

export function dedupeBy(items, keyFn) {
  const seen = new Set();
  return items.filter((item) => {
    const key = keyFn(item);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function createCandidateCollector(target, seen = new Set()) {
  return (candidate) => {
    if (!candidate) return;
    const key = candidate.points.map((point) => `${point.x},${point.y}`).join("|");
    if (seen.has(key)) return;
    seen.add(key);
    target.push(candidate);
  };
}

// Tiered mount-cost weights. Gaps between tiers are wide enough that no lower
// tier can outweigh a higher one across realistic diagram sizes (E < ~200).
// Single WEIGHTED-SUM objective. No tiers: priority is expressed by magnitude. After the two
// inviolable hard violations (route/endpoint through a node) and the egregious overlaps, the heavy
// hitters are ordered, per the maintainer: DOGLEGS > CROSSINGS > CROWDING. Doglegs are always
// avoidable so they are priced highest (a dogleg-y bundle must lose to a clean split); a crossing is
// next; crowding is third — it matters, but a crossing is never traded to relieve it. Everything
// below (bends, length) is fine-grained polish.
export const MOUNT_COST = {
  collision: 1_000_000_000,        // inviolable — a route through a non-endpoint node
  endpointTraversal: 1_000_000_000,// inviolable — an endpoint stub crossing a node
  repeatedCrossing: 5_000_000,     // egregious — the same pair crossing 2+ times
  selfOverlap: 5_000_000,          // egregious — a route overlapping itself
  sharedSegment: 200_000,          // two edges drawn as one line (per overlapping pair)
  sharedSegmentLength: 1_500,      //   …per px of that overlap
  dogleg: 3_300,                   // #1 DOGLEG — a jog against travel direction; always avoidable, priced just above a crossing
  shallowJog: 3_300,               // #1 DOGLEG — the visible small (<36px) stair-step; always avoidable by aligning mounts
  monotonicBacktrack: 3_300,       // #1 DOGLEG — a route doubling back on itself
  perimeterFallback: 4_200,        // a forced detour around a node's perimeter (dogleg-class)
  crossing: 3_000,                 // #2 CROSSING — one honest crossing
  intentMismatch: 1_500,           // ~250px — mounting on the side facing AWAY from the partner (the far-edge wrap)
  overCapacity: 1_000,             // ~167px per excess mount — SOFT: mild over-subscription is tolerated
  cramped: 120,                    // #3 CROWDING — per unit a gap is below MIN_LEGIBLE_GAP; below a crossing (24 units < one crossing) so crossings are never traded for it
  bend: 900,                       // ~50px — a single corner (polish)
  length: 6                        // base unit — per px of wire (polish)
};

export const MIN_LEGIBLE_GAP = 12;  // px; mounts closer than this read as one line
export const MOUNT_MAX_ITERS = 8;   // aggregate-pass convergence bound
export const RECIPROCAL_PARALLEL_OFFSET = 12; // px; lane gap when running a reciprocal return parallel to its request

// Per-node-pair reciprocal gutter bridge (request on an inner lane, return nested outside it).
export const BRIDGE_MOUNT_OFFSET = 9;      // px; request/return mount offset from surface centre
export const BRIDGE_GUTTER_CLEARANCE = 14; // px; gap from the node edge to the inner (request) lane
export const BRIDGE_LANE_GAP = 14;         // px; gap between the request lane and the outer return lane
export const BRIDGE_MAX_LANES = 8;         // how many progressively-higher gutter lanes to try per side
