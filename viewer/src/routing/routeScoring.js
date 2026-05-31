import { rectDistance } from "./routeGeometry.js";
import { PORT_STUB, sideVector } from "./routePorts.js";
import { rectCenter } from "./routeConstants.js";
import { deriveRouteIntent, expectedFacingSides, semanticSurfaceOptions } from "./routeIntent.js";

const FACING_SURFACE_ROLES = new Set(["process", "request", "return", "async", "persistence"]);

export function totalQualityCost(qualityCosts) {
  return Object.values(qualityCosts).reduce((sum, value) => sum + value, 0);
}

export function withQualityCosts(route, qualityCosts) {
  const normalizedQualityCosts = {
    lengthCost: 0,
    boundaryCost: 0,
    nodeClearanceCost: 0,
    edgeProximityCost: 0,
    labelNodeClearanceCost: 0,
    pointCountCost: 0,
    bendCost: 0,
    doglegCost: 0,
    perimeterFallbackCost: 0,
    perimeterLengthCost: 0,
    directnessReward: 0,
    crossingCost: 0,
    repeatedCrossingCost: 0,
    selfOverlapCost: 0,
    routeOverlapCost: 0,
    monotonicBacktrackCost: 0,
    fanOutDirectionCost: 0,
    endpointStackCost: 0,
    splineSideDirectionCost: 0,
    splineStraightnessCost: 0,
    sameLaneExteriorCost: 0,
    ...qualityCosts
  };
  return {
    ...route,
    qualityCosts: normalizedQualityCosts,
    cost: totalQualityCost(normalizedQualityCosts)
  };
}

export function scoreRouteCandidates(candidateList, context) {
  const expectedSides = context.relationship
    ? deriveRouteIntent({
        relationship: context.relationship,
        fromRect: context.fromRect,
        toRect: context.toRect,
        fromLaneIndex: context.fromLaneIndex,
        toLaneIndex: context.toLaneIndex,
        fromRowIndex: context.fromRowIndex,
        toRowIndex: context.toRowIndex
      })
    : expectedFacingSides(context.fromRect, context.toRect);
  candidateList.forEach((candidate) => {
    const travelsTop = candidate.samples.some((point) => point.y < context.topLimit - 4);
    const travelsBottom = candidate.samples.some((point) => point.y > context.bottomLimit + 4);
    candidate.collisions = context.collisionCount(candidate, context.fromId, context.toId, 0);
    candidate.paddedCollisions = context.collisionCount(candidate, context.fromId, context.toId, 8);
    candidate.endpointNodeTraversals = endpointNodeTraversalCount(candidate, context.fromRect, context.toRect);
    const selfOverlapStats = candidate.style === "spline" ? { count: 0, length: 0 } : selfOverlapSegmentStats(candidate.points);
    const crossingStats = candidate.style === "spline" ? { total: 0, repeated: 0 } : context.routeIndex.crossingStats(candidate.points);
    const sharedSegmentStats = candidate.style === "spline" ? { count: 0, length: 0 } : context.routeIndex.sharedSegmentStats(candidate.points);
    candidate.selfOverlappingSegments = selfOverlapStats.count;
    candidate.selfOverlapLength = selfOverlapStats.length;
    candidate.crossings = crossingStats.total;
    candidate.repeatedCrossings = crossingStats.repeated;
    candidate.sharedSegments = sharedSegmentStats.count;
    candidate.sharedSegmentLength = sharedSegmentStats.length;
    const semanticSides = semanticSurfaceOptions({
      expectedSides: {
        source: expectedSides.source ?? expectedSides.expectedSourceSide,
        target: expectedSides.target ?? expectedSides.expectedTargetSide
      },
      relationship: context.relationship,
      fromRect: context.fromRect,
      toRect: context.toRect,
      blockerRects: context.blockerRects,
      canvasWidth: context.canvasWidth,
      canvasHeight: context.canvasHeight
    });
    candidate.surfaceMismatchCount = surfaceMismatchCount(candidate, {
      source: expectedSides.source ?? expectedSides.expectedSourceSide,
      target: expectedSides.target ?? expectedSides.expectedTargetSide
    }, context.relationship);
    candidate.semanticSurfaceMismatchCount = semanticSurfaceMismatchCount(candidate, semanticSides, context.relationship);
    candidate.surfaceDirectionMismatchCount = surfaceDirectionMismatchCount(candidate, context.fromRect, context.toRect, context.relationship);
    candidate.blockedPrimarySurfaceUseCount = blockedPrimarySurfaceUseCount(candidate, {
      source: expectedSides.source ?? expectedSides.expectedSourceSide,
      target: expectedSides.target ?? expectedSides.expectedTargetSide
    }, semanticSides);
    candidate.sameLaneExteriorMismatchCount = sameLaneExteriorMismatchCount(candidate, context);
    candidate.qualityCosts.crossingCost = crossingStats.total * 3000;
    candidate.qualityCosts.repeatedCrossingCost = crossingStats.repeated * 40000;
    candidate.qualityCosts.selfOverlapCost = selfOverlapStats.count * 120000 + selfOverlapStats.length * 1600;
    candidate.qualityCosts.routeOverlapCost = sharedSegmentStats.count * 80000 + sharedSegmentStats.length * 1200;
    candidate.qualityCosts.endpointStackCost = context.routeIndex.hasStackedEndpoint(candidate) ? 90000 : 0;
    candidate.qualityCosts.sameLaneExteriorCost = candidate.sameLaneExteriorMismatchCount * 20000;
    if (context.pairIndex % 2 === 1 && travelsTop) {
      candidate.qualityCosts.fanOutDirectionCost = (candidate.qualityCosts.fanOutDirectionCost ?? 0) + 25000;
    }
    if (context.pairIndex % 2 === 1 && !travelsBottom) {
      candidate.qualityCosts.fanOutDirectionCost = (candidate.qualityCosts.fanOutDirectionCost ?? 0) + 4000;
    }
    if (context.pairIndex % 2 === 0 && travelsBottom) {
      candidate.qualityCosts.fanOutDirectionCost = (candidate.qualityCosts.fanOutDirectionCost ?? 0) + 600;
    }
    candidate.cost = totalQualityCost(candidate.qualityCosts);
  });
}

function surfaceMismatchCount(candidate, expectedSides, relationship) {
  if (candidate.style === "spline") return 0;
  if (relationship?.preferredStartSide || relationship?.preferredEndSide) return 0;
  if (relationship?.kind && !FACING_SURFACE_ROLES.has(relationship.kind)) return 0;
  let count = 0;
  if (candidate.startSide && !sideMatches(candidate.startSide, expectedSides.source)) count += 1;
  if (candidate.endSide && !sideMatches(candidate.endSide, expectedSides.target)) count += 1;
  return count;
}

function sideMatches(side, expected) {
  return expected instanceof Set ? expected.has(side) : side === expected;
}

function semanticSurfaceMismatchCount(candidate, expectedSides, relationship) {
  if (!relationship?.relationshipType && !relationship?.kind && !relationship?.returnOf && !relationship?.outcome && !relationship?.stepId && !relationship?.flowId) return 0;
  return surfaceMismatchCount(candidate, expectedSides, relationship);
}

function blockedPrimarySurfaceUseCount(candidate, primarySides, semanticSides) {
  let count = 0;
  if (semanticSides.source.size > 1 && candidate.startSide === primarySides.source) count += 1;
  if (semanticSides.target.size > 1 && candidate.endSide === primarySides.target) count += 1;
  return count;
}

function surfaceDirectionMismatchCount(candidate, fromRect, toRect, relationship) {
  if (candidate.style === "spline") return 0;
  if (relationship?.preferredStartSide || relationship?.preferredEndSide) return 0;
  if (!candidate.startSide || !candidate.endSide || !fromRect || !toRect) return 0;
  const fromCenter = rectCenter(fromRect);
  const toCenter = rectCenter(toRect);
  const direction = {
    x: toCenter.x - fromCenter.x,
    y: toCenter.y - fromCenter.y
  };
  if (direction.x === 0 && direction.y === 0) return 0;
  const startVector = sideVector(candidate.startSide);
  const endVector = sideVector(candidate.endSide);
  let count = 0;
  if (startVector.x * direction.x + startVector.y * direction.y < 0) count += 1;
  if (endVector.x * direction.x + endVector.y * direction.y > 0) count += 1;
  return count;
}

function routeSegments(points) {
  const segments = [];
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    if (start.y === end.y) {
      segments.push({ orientation: "horizontal", line: start.y, min: Math.min(start.x, end.x), max: Math.max(start.x, end.x) });
    } else if (start.x === end.x) {
      segments.push({ orientation: "vertical", line: start.x, min: Math.min(start.y, end.y), max: Math.max(start.y, end.y) });
    }
  }
  return segments;
}

function sameLaneExteriorMismatchCount(candidate, context) {
  if (candidate.style === "spline") return 0;
  if (!context.relationship?.relationshipType && !context.relationship?.kind && !context.relationship?.returnOf && !context.relationship?.outcome && !context.relationship?.stepId && !context.relationship?.flowId) return 0;
  if (context.fromLaneIndex !== context.toLaneIndex || context.fromRowIndex === context.toRowIndex) return 0;
  if (!context.canvasWidth || !context.fromRect || !context.toRect) return 0;
  const nodeLeft = Math.min(context.fromRect.x, context.toRect.x);
  const nodeRight = Math.max(context.fromRect.x + context.fromRect.width, context.toRect.x + context.toRect.width);
  const nodeCenterX = (context.fromRect.x + context.fromRect.width / 2 + context.toRect.x + context.toRect.width / 2) / 2;
  const preferLeftExterior = nodeCenterX < context.canvasWidth / 2;
  const verticalSegments = routeSegments(candidate.points ?? []).filter((segment) => segment.orientation === "vertical");
  const usesLeftExterior = verticalSegments.some((segment) => segment.line < nodeLeft);
  const usesRightExterior = verticalSegments.some((segment) => segment.line > nodeRight);
  if (preferLeftExterior) return usesLeftExterior ? 0 : 1;
  return usesRightExterior ? 0 : 1;
}

function selfOverlapSegmentStats(points) {
  const segments = routeSegments(points ?? []);
  let count = 0;
  let length = 0;
  for (let leftIndex = 0; leftIndex < segments.length; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < segments.length; rightIndex += 1) {
      const left = segments[leftIndex];
      const right = segments[rightIndex];
      if (left.orientation !== right.orientation || left.line !== right.line) continue;
      const overlap = Math.min(left.max, right.max) - Math.max(left.min, right.min);
      if (overlap > 1) {
        count += 1;
        length += overlap;
      }
    }
  }
  return { count, length };
}

function pointInsideRect(point, rect) {
  return point.x > rect.x && point.x < rect.x + rect.width && point.y > rect.y && point.y < rect.y + rect.height;
}

function endpointNodeTraversalCount(candidate, fromRect, toRect) {
  const traversed = new Set();
  for (const sample of candidate.samples ?? []) {
    if (fromRect && pointInsideRect(sample, fromRect)) traversed.add("from");
    if (toRect && pointInsideRect(sample, toRect)) traversed.add("to");
  }
  return traversed.size;
}

function hasQualityCost(candidate, costName) {
  return (candidate.qualityCosts?.[costName] ?? 0) > 0 ? 1 : 0;
}

function routeMetric(candidate, metricName) {
  return candidate[metricName] ?? 0;
}

export function compareByRoutePriority(a, b) {
  return (
    routeMetric(a, "collisions") - routeMetric(b, "collisions") ||
    routeMetric(a, "endpointNodeTraversals") - routeMetric(b, "endpointNodeTraversals") ||
    routeMetric(a, "selfOverlappingSegments") - routeMetric(b, "selfOverlappingSegments") ||
    routeMetric(a, "selfOverlapLength") - routeMetric(b, "selfOverlapLength") ||
    routeMetric(a, "repeatedCrossings") - routeMetric(b, "repeatedCrossings") ||
    routeMetric(a, "semanticSurfaceMismatchCount") - routeMetric(b, "semanticSurfaceMismatchCount") ||
    routeMetric(a, "blockedPrimarySurfaceUseCount") - routeMetric(b, "blockedPrimarySurfaceUseCount") ||
    routeMetric(a, "surfaceDirectionMismatchCount") - routeMetric(b, "surfaceDirectionMismatchCount") ||
    routeMetric(a, "sameLaneExteriorMismatchCount") - routeMetric(b, "sameLaneExteriorMismatchCount") ||
    routeMetric(a, "paddedCollisions") - routeMetric(b, "paddedCollisions") ||
    routeMetric(a, "sharedSegments") - routeMetric(b, "sharedSegments") ||
    routeMetric(a, "sharedSegmentLength") - routeMetric(b, "sharedSegmentLength") ||
    // A perimeter-fallback detour is a worse outcome than a few honest crossings,
    // so its presence outranks the crossing count: prefer a short interior route
    // that crosses a couple of lines over a long swing around the diagram edge.
    hasQualityCost(a, "perimeterFallbackCost") - hasQualityCost(b, "perimeterFallbackCost") ||
    routeMetric(a, "crossings") - routeMetric(b, "crossings") ||
    // An offered gutter escape is a soft option, not a mandate: only prefer leaving a
    // blocked facing surface as a tiebreak AFTER honest crossings are compared, so the
    // escape must earn its place by producing a genuinely cleaner route.
    routeMetric(a, "blockedPrimarySurfaceUseCount") - routeMetric(b, "blockedPrimarySurfaceUseCount") ||
    hasQualityCost(a, "monotonicBacktrackCost") - hasQualityCost(b, "monotonicBacktrackCost") ||
    hasQualityCost(a, "endpointStackCost") - hasQualityCost(b, "endpointStackCost") ||
    routeMetric(a, "bends") - routeMetric(b, "bends") ||
    routeMetric(a, "cost") - routeMetric(b, "cost")
  );
}

export function sortedRouteCandidates(candidateList) {
  return candidateList.sort(compareByRoutePriority);
}

export function isCleanRouteCandidate(candidate) {
  return (
    candidate.collisions === 0 &&
    candidate.paddedCollisions === 0 &&
    candidate.endpointNodeTraversals === 0 &&
    candidate.selfOverlappingSegments === 0 &&
    candidate.repeatedCrossings === 0 &&
    candidate.crossings === 0 &&
    candidate.sharedSegments === 0 &&
    candidate.qualityCosts.endpointStackCost === 0 &&
    candidate.qualityCosts.perimeterFallbackCost === 0 &&
    candidate.qualityCosts.doglegCost === 0
  );
}

export function warningRouteCandidate(candidate, context) {
  const warnings = [];
  const leastBad = context.style === "spline"
    ? candidate.collisions > 0
    : candidate.collisions > 0 || candidate.paddedCollisions > 0;
  if (leastBad) {
    warnings.push({
      code: "least-bad-route",
      message: context.style === "spline"
        ? "No clean spline route was available for the current node arrangement."
        : context.style === "straight"
          ? "No clean straight route was available for the current node arrangement."
          : "No clean route was available for the current node arrangement."
    });
  }
  if (context.style === "orthogonal" && candidate.endpointNodeTraversals > 0) {
    warnings.push({
      code: "endpoint-node-traversal",
      message: "Selected route crosses through its source or target node interior."
    });
  }
  if (context.style === "orthogonal" && candidate.selfOverlappingSegments > 0) {
    warnings.push({
      code: "self-overlapping-route",
      message: "Selected route doubles back over its own line."
    });
  }
  if (context.style === "orthogonal" && candidate.repeatedCrossings > 0) {
    warnings.push({
      code: "repeated-route-crossing",
      message: "Selected route crosses the same existing route more than once."
    });
  }
  if (context.style === "orthogonal" && candidate.qualityCosts.perimeterFallbackCost > 0) {
    warnings.push({
      code: "perimeter-fallback-route",
      message: "Selected route used a perimeter fallback instead of an interior corridor."
    });
  }
  if (rectDistance(context.fromRect, context.toRect) < PORT_STUB * 2) {
    warnings.push({
      code: "nodes-too-close",
      message: "Source and target nodes are too close for clean connector routing."
    });
  }
  return { ...candidate, warnings };
}
