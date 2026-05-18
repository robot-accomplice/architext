import { clamp } from "./routeGeometry.js";
import { SIDES } from "./routePorts.js";
import {
  isCleanRouteCandidate,
  scoreRouteCandidates,
  sortedRouteCandidates,
  warningRouteCandidate
} from "./routeScoring.js";
import { allSidePairs, candidatePorts, portPairsFor, sidePairsFor } from "./routeCandidatePorts.js";

export function selectRouteCandidate(input) {
  const {
    collisionCount,
    corridors,
    endpointOffsets,
    fromId,
    fromRect,
    index,
    pairIndex,
    relationship,
    routeCandidates,
    routeIndex,
    stats,
    style,
    toId,
    toRect,
    usedRoutes
  } = input;
  const candidates = [];
  const candidateKeys = new Set();
  const addCandidate = (candidate) => {
    if (!candidate) return;
    const key = candidate.points.map((point) => `${point.x},${point.y}`).join("|");
    if (candidateKeys.has(key)) return;
    candidateKeys.add(key);
    candidates.push(candidate);
  };
  const sidePairs = sidePairsFor(fromRect, toRect);
  const routeOffset = pairIndex * 40 + (index % 6) * 14;
  const topLimit = Math.min(fromRect.y, toRect.y);
  const bottomLimit = Math.max(fromRect.y + fromRect.height, toRect.y + toRect.height);
  const scoringContext = {
    fromId,
    toId,
    fromRect,
    toRect,
    pairIndex,
    topLimit,
    bottomLimit,
    routeIndex,
    collisionCount
  };
  const warnCandidate = (candidate) => warningRouteCandidate(candidate, { style, fromRect, toRect });

  if (style === "spline") {
    const splineCandidates = [];
    sidePairs.forEach(([startSide, endSide]) => {
      const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets);
      for (const [startPort, endPort] of portPairsFor(ports)) {
        const start = startPort.anchor;
        const end = endPort.anchor;
        const centerDistance = Math.hypot(end.x - start.x, end.y - start.y);
        const curveOffset = clamp(centerDistance * 0.18 + pairIndex * 8, 36, 180);
        const routeSpread = (index % 7) * 10;
        for (const offset of [
          curveOffset + routeSpread,
          -curveOffset - routeSpread,
          curveOffset * 0.72,
          -curveOffset * 0.72,
          curveOffset * 1.36 + routeSpread,
          -curveOffset * 1.36 - routeSpread,
          curveOffset * 2.1 + routeSpread,
          -curveOffset * 2.1 - routeSpread,
          curveOffset * 0.38,
          -curveOffset * 0.38
        ]) {
          splineCandidates.push(routeCandidates.splineCandidate(relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort, pairIndex, offset));
        }
      }
    });
    scoreRouteCandidates(splineCandidates, scoringContext);
    return sortedRouteCandidates(splineCandidates).map(warnCandidate)[0];
  }

  if (style === "straight") {
    const straightCandidates = [];
    sidePairs.forEach(([startSide, endSide]) => {
      const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets);
      for (const [startPort, endPort] of portPairsFor(ports)) {
        straightCandidates.push(routeCandidates.straightCandidate(relationship, fromId, toId, usedRoutes, startPort, endPort));
      }
    });
    scoreRouteCandidates(straightCandidates, scoringContext);
    return sortedRouteCandidates(straightCandidates).map(warnCandidate)[0];
  }

  const cheapCandidates = [];
  const addCheapCandidate = (candidate) => {
    if (!candidate) return;
    const key = candidate.points.map((point) => `${point.x},${point.y}`).join("|");
    if (candidateKeys.has(key)) return;
    candidateKeys.add(key);
    cheapCandidates.push(candidate);
  };

  allSidePairs().forEach(([startSide, endSide]) => {
    const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets);
    for (const [startPort, endPort] of portPairsFor(ports)) {
      if (pairIndex === 0) {
        addCheapCandidate(routeCandidates.directPortCandidate(relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort));
      }
      for (const corridor of corridors) {
        addCheapCandidate(routeCandidates.corridorCandidate(relationship, fromId, toId, usedRoutes, startPort, endPort, corridor));
      }
    }
  });

  scoreRouteCandidates(cheapCandidates, scoringContext);
  const hasCleanCheapCandidate = cheapCandidates.some(isCleanRouteCandidate);
  if (stats) {
    stats.edgesPlanned = (stats.edgesPlanned ?? 0) + 1;
    stats.cheapCandidateCount = (stats.cheapCandidateCount ?? 0) + cheapCandidates.length;
    if (!hasCleanCheapCandidate) {
      stats.gridEscalations = (stats.gridEscalations ?? 0) + 1;
      const bestCheap = sortedRouteCandidates([...cheapCandidates])[0];
      const reasons = stats.cheapRejectionReasons ?? {};
      if (bestCheap) {
        if (bestCheap.collisions > 0) reasons.collisions = (reasons.collisions ?? 0) + 1;
        if (bestCheap.paddedCollisions > 0) reasons.paddedCollisions = (reasons.paddedCollisions ?? 0) + 1;
        if (bestCheap.repeatedCrossings > 0) reasons.repeatedCrossings = (reasons.repeatedCrossings ?? 0) + 1;
        if (bestCheap.crossings > 0) reasons.crossings = (reasons.crossings ?? 0) + 1;
        if (bestCheap.qualityCosts.endpointStackCost > 0) reasons.endpointStack = (reasons.endpointStack ?? 0) + 1;
        if (bestCheap.qualityCosts.doglegCost > 0) reasons.dogleg = (reasons.dogleg ?? 0) + 1;
      } else {
        reasons.noCandidate = (reasons.noCandidate ?? 0) + 1;
      }
      stats.cheapRejectionReasons = reasons;
    }
  }
  if (hasCleanCheapCandidate) {
    candidates.push(...cheapCandidates);
  } else {
    candidates.push(...cheapCandidates);
    sidePairs.forEach(([startSide, endSide]) => {
      const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets, "grid");
      for (const [startPort, endPort] of portPairsFor(ports)) {
        addCandidate(routeCandidates.gridRoute(relationship, fromId, toId, startSide, endSide, routeOffset, usedRoutes, startPort, endPort));
      }
    });
  }

  if (!hasCleanCheapCandidate) {
    SIDES.forEach((side) => {
      const ports = candidatePorts(fromRect, toRect, side, side, endpointOffsets);
      for (const [startPort, endPort] of portPairsFor(ports)) {
        addCandidate(routeCandidates.perimeterRoute(relationship, fromId, toId, side, routeOffset, usedRoutes, startPort, endPort));
        for (const perimeterCandidate of routeCandidates.cornerPerimeterRoutes(relationship, fromId, toId, routeOffset, usedRoutes, startPort, endPort)) {
          addCandidate(perimeterCandidate);
        }
      }
    });
  }

  scoreRouteCandidates(candidates.filter((candidate) => candidate.collisions === undefined), scoringContext);

  return sortedRouteCandidates(candidates).map(warnCandidate)[0];
}
