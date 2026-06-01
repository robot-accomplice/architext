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
// Single WEIGHTED-SUM objective. No tiers: each weight is a defect's worth in px-of-detour
// (length is the base unit, 6/px). Priority is expressed by magnitude. The two hard violations
// stay effectively inviolable by sheer scale; everything else trades against length so the
// optimizer prefers the SHORTER, straighter, facing-side route — and removes doglegs, which are
// always avoidable, by pricing them as high as a crossing.
export const MOUNT_COST = {
  collision: 1_000_000_000,        // inviolable — a route through a non-endpoint node
  endpointTraversal: 1_000_000_000,// inviolable — an endpoint stub crossing a node
  repeatedCrossing: 5_000_000,     // egregious — the same pair crossing 2+ times
  selfOverlap: 5_000_000,          // egregious — a route overlapping itself
  sharedSegment: 200_000,          // two edges drawn as one line (per overlapping pair)
  sharedSegmentLength: 1_500,      //   …per px of that overlap
  perimeterFallback: 4_200,        // ~700px — a forced detour around a node's perimeter
  monotonicBacktrack: 4_200,       // ~700px — a route doubling back on itself
  crossing: 3_000,                 // ~500px — one honest crossing (anchor; preserves crossing minimisation)
  dogleg: 3_000,                   // ~500px — a jog against travel direction; always avoidable, so priced like a crossing to force its removal
  shallowJog: 3_000,               // ~500px — a small (<36px) stair-step; the visible "weird dogleg", always avoidable by aligning the two mounts
  intentMismatch: 1_500,           // ~250px — mounting on the side facing AWAY from the partner (the far-edge wrap)
  overCapacity: 1_000,             // ~167px per excess mount — SOFT: mild over-subscription is tolerated
  bend: 300,                       // ~50px — a single corner
  cramped: 80,                     // ~13px per unit a gap is below MIN_LEGIBLE_GAP — minor; never outweighs a crossing
  length: 6                        // base unit — per px of wire (raised from 3: wire length was under-penalised)
};

export const MIN_LEGIBLE_GAP = 12;  // px; mounts closer than this read as one line
export const MOUNT_MAX_ITERS = 8;   // aggregate-pass convergence bound
export const RECIPROCAL_PARALLEL_OFFSET = 12; // px; lane gap when running a reciprocal return parallel to its request

// Per-node-pair reciprocal gutter bridge (request on an inner lane, return nested outside it).
export const BRIDGE_MOUNT_OFFSET = 9;      // px; request/return mount offset from surface centre
export const BRIDGE_GUTTER_CLEARANCE = 14; // px; gap from the node edge to the inner (request) lane
export const BRIDGE_LANE_GAP = 14;         // px; gap between the request lane and the outer return lane
export const BRIDGE_MAX_LANES = 8;         // how many progressively-higher gutter lanes to try per side
