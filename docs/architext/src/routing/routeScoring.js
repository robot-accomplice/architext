import { rectDistance } from "./routeGeometry.js";
import { PORT_STUB } from "./routePorts.js";

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
    routeOverlapCost: 0,
    monotonicBacktrackCost: 0,
    fanOutDirectionCost: 0,
    endpointStackCost: 0,
    splineSideDirectionCost: 0,
    splineStraightnessCost: 0,
    ...qualityCosts
  };
  return {
    ...route,
    qualityCosts: normalizedQualityCosts,
    cost: totalQualityCost(normalizedQualityCosts)
  };
}

export function scoreRouteCandidates(candidateList, context) {
  candidateList.forEach((candidate) => {
    const travelsTop = candidate.samples.some((point) => point.y < context.topLimit - 4);
    const travelsBottom = candidate.samples.some((point) => point.y > context.bottomLimit + 4);
    candidate.collisions = context.collisionCount(candidate, context.fromId, context.toId, 0);
    candidate.paddedCollisions = context.collisionCount(candidate, context.fromId, context.toId, 8);
    const crossingStats = candidate.style === "spline" ? { total: 0, repeated: 0 } : context.routeIndex.crossingStats(candidate.points);
    const sharedSegmentStats = candidate.style === "spline" ? { count: 0, length: 0 } : context.routeIndex.sharedSegmentStats(candidate.points);
    candidate.crossings = crossingStats.total;
    candidate.repeatedCrossings = crossingStats.repeated;
    candidate.sharedSegments = sharedSegmentStats.count;
    candidate.sharedSegmentLength = sharedSegmentStats.length;
    candidate.qualityCosts.crossingCost = crossingStats.total * 3000;
    candidate.qualityCosts.repeatedCrossingCost = crossingStats.repeated * 40000;
    candidate.qualityCosts.routeOverlapCost = sharedSegmentStats.count * 80000 + sharedSegmentStats.length * 1200;
    candidate.qualityCosts.endpointStackCost = context.routeIndex.hasStackedEndpoint(candidate) ? 90000 : 0;
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

export function sortedRouteCandidates(candidateList) {
  return candidateList.sort((a, b) =>
    a.collisions - b.collisions ||
    a.paddedCollisions - b.paddedCollisions ||
    a.repeatedCrossings - b.repeatedCrossings ||
    a.crossings - b.crossings ||
    (a.qualityCosts.monotonicBacktrackCost > 0 ? 1 : 0) - (b.qualityCosts.monotonicBacktrackCost > 0 ? 1 : 0) ||
    (a.qualityCosts.endpointStackCost > 0 ? 1 : 0) - (b.qualityCosts.endpointStackCost > 0 ? 1 : 0) ||
    (a.qualityCosts.perimeterFallbackCost > 0 ? 1 : 0) - (b.qualityCosts.perimeterFallbackCost > 0 ? 1 : 0) ||
    a.bends - b.bends ||
    a.cost - b.cost
  );
}

export function isCleanRouteCandidate(candidate) {
  return (
    candidate.collisions === 0 &&
    candidate.paddedCollisions === 0 &&
    candidate.repeatedCrossings === 0 &&
    candidate.crossings === 0 &&
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
