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
export const MOUNT_COST = {
  collision: 1_000_000_000,        // tier 0 — inviolable
  overCapacity: 1_000_000_000,     // tier 0
  endpointTraversal: 1_000_000_000,// tier 0
  repeatedCrossing: 5_000_000,     // tier 1
  selfOverlap: 5_000_000,          // tier 1
  sharedSegment: 200_000,          // tier 2 (per overlapping pair)
  sharedSegmentLength: 1_500,      // tier 2 (per unit length)
  crossing: 3_000,                 // tier 3 (matches existing crossingCost)
  bend: 420,                       // tier 4 (matches ROUTE_COST_WEIGHTS.bend)
  dogleg: 1_500,                   // tier 4 (per reversing segment; below crossing, above a bend)
  cramped: 1_200,                  // tier 5 (per unit a gap is below MIN_LEGIBLE_GAP)
  intentMismatch: 900,             // tier 5 (per endpoint leaving the non-facing side)
  length: 3                        // tier 5 (per unit of wire length — prefers shorter routes; tunable)
};

export const MIN_LEGIBLE_GAP = 12;  // px; mounts closer than this read as one line
export const MOUNT_MAX_ITERS = 8;   // aggregate-pass convergence bound
