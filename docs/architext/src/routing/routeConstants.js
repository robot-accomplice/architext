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
