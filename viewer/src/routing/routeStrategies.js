import { bendCount, clamp, lineSamples } from "./routeGeometry.js";
import { SIDES } from "./routePorts.js";
import { pathToSvg, simplifyOrthogonalPoints } from "./routeRendering.js";
import {
  isCleanRouteCandidate,
  scoreRouteCandidates,
  sortedRouteCandidates,
  withQualityCosts,
  warningRouteCandidate
} from "./routeScoring.js";
import { candidatePorts, portPairsFor, sidePairsFor } from "./routeCandidatePorts.js";
import { ROUTE_COST_WEIGHTS, ROUTE_SPACING, SPLINE_CURVE_VARIANTS, createCandidateCollector, dedupeBy } from "./routeConstants.js";

function preferredStartSidePairs(pairs, relationship) {
  if (!relationship.preferredStartSide && !relationship.preferredEndSide) return pairs;
  const preferred = pairs.filter(([startSide, endSide]) => (
    (!relationship.preferredStartSide || startSide === relationship.preferredStartSide) &&
    (!relationship.preferredEndSide || endSide === relationship.preferredEndSide)
  ));
  return preferred.length > 0 ? preferred : pairs;
}

function semanticFlowRelationship(relationship) {
  return Boolean(
    relationship.relationshipType === "flow" ||
    relationship.kind ||
    relationship.returnOf ||
    relationship.outcome ||
    relationship.stepId ||
    relationship.flowId
  );
}

function routeSidePairsFor(fromRect, toRect, relationship) {
  const basePairs = sidePairsFor(fromRect, toRect);
  if (!semanticFlowRelationship(relationship)) return basePairs;
  const seen = new Set(basePairs.map(([startSide, endSide]) => `${startSide}:${endSide}`));
  const expandedPairs = [...basePairs];
  for (const startSide of SIDES) {
    for (const endSide of SIDES) {
      const key = `${startSide}:${endSide}`;
      if (seen.has(key)) continue;
      seen.add(key);
      expandedPairs.push([startSide, endSide]);
    }
  }
  return expandedPairs;
}

function isSideAvailable(endpointSideUsage, nodeId, rect, side) {
  return !endpointSideUsage || endpointSideUsage.isAvailable(nodeId, side, rect);
}

function isSidePairAvailable(input, startSide, endSide) {
  return (
    isSideAvailable(input.endpointSideUsage, input.fromId, input.fromRect, startSide) &&
    isSideAvailable(input.endpointSideUsage, input.toId, input.toRect, endSide)
  );
}

function availableSidePairs(pairs, input) {
  return pairs.filter(([startSide, endSide]) => isSidePairAvailable(input, startSide, endSide));
}

function fixedPreferredOrthogonalCandidate(relationship, fromRect, toRect, endpointOffsets, routeCandidates, usedRoutes, input) {
  if (!fromRect.fixedPorts || !relationship.preferredStartSide) return null;
  const endSide = relationship.preferredEndSide ?? sidePairsFor(fromRect, toRect)[0]?.[1] ?? "left";
  if (input && !isSidePairAvailable(input, relationship.preferredStartSide, endSide)) return null;
  const ports = candidatePorts(fromRect, toRect, relationship.preferredStartSide, endSide, endpointOffsets);
  const [startPort, endPort] = portPairsFor(ports)[0] ?? [];
  if (!startPort || !endPort) return null;
  if (relationship.preferredStartSide === "left" && endSide === "bottom") {
    const gutter = Math.min(startPort.port.x, toRect.x) - ROUTE_COST_WEIGHTS.fixedPreferredGutter;
    const points = simplifyOrthogonalPoints([
      startPort.anchor,
      startPort.port,
      { x: gutter, y: startPort.port.y },
      { x: gutter, y: endPort.port.y },
      endPort.port,
      endPort.anchor
    ]);
    const samples = lineSamples(points);
    const label = samples[Math.floor(samples.length / 2)] ?? points[Math.floor(points.length / 2)] ?? startPort.anchor;
    return withQualityCosts({
      d: pathToSvg(points),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(points),
      samples,
      points
    }, {
      lengthCost: samples.reduce((sum, sample, index) => index === 0 ? 0 : sum + Math.hypot(sample.x - samples[index - 1].x, sample.y - samples[index - 1].y), 0),
      pointCountCost: points.length * ROUTE_COST_WEIGHTS.pointCount,
      bendCost: bendCount(points) * ROUTE_COST_WEIGHTS.bend
    });
  }
  const preferred = routeCandidates.directPortCandidate(relationship, relationship.from, relationship.to, relationship.preferredStartSide, endSide, usedRoutes, startPort, endPort);
  if (preferred) return preferred;
  if (relationship.preferredStartSide === endSide) {
    const gutter = endSide === "right"
      ? toRect.x + toRect.width + ROUTE_COST_WEIGHTS.fixedPreferredGutter
      : endSide === "left"
        ? toRect.x - ROUTE_COST_WEIGHTS.fixedPreferredGutter
        : endSide === "top"
          ? toRect.y - ROUTE_COST_WEIGHTS.fixedPreferredGutter
          : toRect.y + toRect.height + ROUTE_COST_WEIGHTS.fixedPreferredGutter;
    const points = simplifyOrthogonalPoints(
      endSide === "left" || endSide === "right"
        ? [startPort.anchor, startPort.port, { x: gutter, y: startPort.port.y }, { x: gutter, y: endPort.port.y }, endPort.port, endPort.anchor]
        : [startPort.anchor, startPort.port, { x: startPort.port.x, y: gutter }, { x: endPort.port.x, y: gutter }, endPort.port, endPort.anchor]
    );
    const samples = lineSamples(points);
    const label = samples[Math.floor(samples.length / 2)] ?? points[Math.floor(points.length / 2)] ?? startPort.anchor;
    return withQualityCosts({
      d: pathToSvg(points),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(points),
      samples,
      points
    }, {
      lengthCost: samples.reduce((sum, sample, index) => index === 0 ? 0 : sum + Math.hypot(sample.x - samples[index - 1].x, sample.y - samples[index - 1].y), 0),
      pointCountCost: points.length * ROUTE_COST_WEIGHTS.pointCount,
      bendCost: bendCount(points) * ROUTE_COST_WEIGHTS.bend
    });
  }
  const points = simplifyOrthogonalPoints(
    relationship.preferredStartSide === "left" || relationship.preferredStartSide === "right"
      ? [startPort.anchor, startPort.port, { x: endPort.port.x, y: startPort.port.y }, endPort.port, endPort.anchor]
      : [startPort.anchor, startPort.port, { x: startPort.port.x, y: endPort.port.y }, endPort.port, endPort.anchor]
  );
  const samples = lineSamples(points);
  const label = samples[Math.floor(samples.length / 2)] ?? points[Math.floor(points.length / 2)] ?? startPort.anchor;
  return withQualityCosts({
    d: pathToSvg(points),
    labelX: label.x,
    labelY: label.y,
    bends: bendCount(points),
    samples,
    points
  }, {
    lengthCost: samples.reduce((sum, sample, index) => index === 0 ? 0 : sum + Math.hypot(sample.x - samples[index - 1].x, sample.y - samples[index - 1].y), 0),
    pointCountCost: points.length * ROUTE_COST_WEIGHTS.pointCount,
    bendCost: bendCount(points) * ROUTE_COST_WEIGHTS.bend
  });
}

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
    progressTick,
    style,
    toId,
    toRect,
    usedRoutes,
    canvasWidth,
    canvasHeight,
    blockerRects
  } = input;
  const candidates = [];
  const candidateKeys = new Set();
  const addCandidate = createCandidateCollector(candidates, candidateKeys);
  const baseSidePairs = routeSidePairsFor(fromRect, toRect, relationship);
  const sidePairs = preferredStartSidePairs(availableSidePairs(baseSidePairs, input), relationship);
  const fallbackSidePairs = sidePairs.length > 0 ? sidePairs : preferredStartSidePairs(baseSidePairs, relationship);
  const routeOffset = pairIndex * ROUTE_SPACING.pairOffset + (index % ROUTE_SPACING.indexOffsetModulo) * ROUTE_SPACING.indexOffset;
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
    collisionCount,
    relationship,
    fromLaneIndex: input.fromLaneIndex,
    toLaneIndex: input.toLaneIndex,
    fromRowIndex: input.fromRowIndex,
    toRowIndex: input.toRowIndex,
    canvasWidth,
    canvasHeight,
    blockerRects
  };
  const warnCandidate = (candidate) => warningRouteCandidate(candidate, { style, fromRect, toRect });
  const relaxedPreferenceRoute = () => (
    relationship.preferredStartSide || relationship.preferredEndSide
      ? selectRouteCandidate({
          ...input,
          relationship: {
            ...relationship,
            preferredStartSide: undefined,
            preferredEndSide: undefined
          }
        })
      : undefined
  );

  if (style === "spline") {
    const splineCandidates = [];
    fallbackSidePairs.forEach(([startSide, endSide]) => {
      const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets);
      for (const [startPort, endPort] of portPairsFor(ports)) {
        const start = startPort.anchor;
        const end = endPort.anchor;
        const centerDistance = Math.hypot(end.x - start.x, end.y - start.y);
        const curveOffset = clamp(centerDistance * 0.18 + pairIndex * ROUTE_SPACING.splinePairOffset, ROUTE_SPACING.splineMinCurve, ROUTE_SPACING.splineMaxCurve);
        const routeSpread = (index % ROUTE_SPACING.splineSpreadModulo) * ROUTE_SPACING.splineSpread;
        for (const variant of SPLINE_CURVE_VARIANTS) {
          const offset = curveOffset * variant.multiplier + routeSpread * variant.spread;
          splineCandidates.push(routeCandidates.splineCandidate(relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort, pairIndex, offset));
        }
      }
    });
    scoreRouteCandidates(splineCandidates, scoringContext);
    return sortedRouteCandidates(splineCandidates).map(warnCandidate)[0] ?? relaxedPreferenceRoute();
  }

  if (style === "straight") {
    const straightCandidates = [];
    fallbackSidePairs.forEach(([startSide, endSide]) => {
      const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets);
      for (const [startPort, endPort] of portPairsFor(ports)) {
        straightCandidates.push(routeCandidates.straightCandidate(relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort));
      }
    });
    scoreRouteCandidates(straightCandidates, scoringContext);
    return sortedRouteCandidates(straightCandidates).map(warnCandidate)[0] ?? relaxedPreferenceRoute();
  }

  const fixedPreferredRoute = fixedPreferredOrthogonalCandidate(relationship, fromRect, toRect, endpointOffsets, routeCandidates, usedRoutes, input);
  if (fixedPreferredRoute) {
    scoreRouteCandidates([fixedPreferredRoute], scoringContext);
    return warnCandidate(fixedPreferredRoute);
  }

  const cheapCandidates = [];
  const addCheapCandidate = createCandidateCollector(cheapCandidates, candidateKeys);

  fallbackSidePairs.forEach(([startSide, endSide]) => {
    const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets);
    for (const [startPort, endPort] of portPairsFor(ports)) {
      if (pairIndex === 0) {
        addCheapCandidate(routeCandidates.directPortCandidate(relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort));
      }
      for (const corridor of corridors) {
        addCheapCandidate(routeCandidates.corridorCandidate(relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort, corridor));
      }
    }
  });

  scoreRouteCandidates(cheapCandidates, scoringContext);
  progressTick?.();
  const hasCleanCheapCandidate = cheapCandidates.some(isCleanRouteCandidate);
  const hasCleanSemanticCheapCandidate = cheapCandidates.some((candidate) =>
    isCleanRouteCandidate(candidate) && (candidate.semanticSurfaceMismatchCount ?? 0) === 0
  );
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
  if (hasCleanCheapCandidate && hasCleanSemanticCheapCandidate) {
    candidates.push(...cheapCandidates);
  } else {
    candidates.push(...cheapCandidates);
    fallbackSidePairs.forEach(([startSide, endSide]) => {
      const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets, "grid");
      for (const [startPort, endPort] of portPairsFor(ports)) {
        addCandidate(routeCandidates.gridRoute(relationship, fromId, toId, startSide, endSide, routeOffset, usedRoutes, startPort, endPort));
      }
    });
  }

  if (!hasCleanCheapCandidate) {
    const availablePerimeterSides = SIDES.filter((side) => isSidePairAvailable(input, side, side));
    const preferredPerimeterSide = relationship.preferredStartSide && isSidePairAvailable(input, relationship.preferredStartSide, relationship.preferredStartSide)
      ? [relationship.preferredStartSide]
      : null;
    const perimeterStartSides = preferredPerimeterSide ?? (availablePerimeterSides.length > 0 ? availablePerimeterSides : SIDES);
    perimeterStartSides.forEach((side) => {
      if (relationship.preferredEndSide && relationship.preferredEndSide !== side) return;
      const ports = candidatePorts(fromRect, toRect, side, side, endpointOffsets);
      for (const [startPort, endPort] of portPairsFor(ports)) {
        addCandidate(routeCandidates.perimeterRoute(relationship, fromId, toId, side, routeOffset, usedRoutes, startPort, endPort));
        for (const perimeterCandidate of routeCandidates.cornerPerimeterRoutes(relationship, fromId, toId, routeOffset, usedRoutes, startPort, endPort)) {
          addCandidate(perimeterCandidate);
        }
      }
    });
  }

  scoreRouteCandidates(dedupeBy(
    candidates.filter((candidate) => candidate.collisions === undefined),
    (candidate) => candidate.points.map((point) => `${point.x},${point.y}`).join("|")
  ), scoringContext);

  return sortedRouteCandidates(candidates).map(warnCandidate)[0]
    ?? fixedPreferredOrthogonalCandidate(relationship, fromRect, toRect, endpointOffsets, routeCandidates, usedRoutes, input)
    ?? relaxedPreferenceRoute();
}
