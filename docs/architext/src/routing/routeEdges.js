import { pathToSvgWithHops } from "./routeRendering.js";
import { createRouteIndex } from "./routeIndex.js";
import {
  bendCount,
  boundsForPoints,
  distanceToRect,
  distanceToRectSquared,
  lineSamples,
  nearestSample,
  rectsOverlap,
  sampleCubic,
  sampleLine,
  segmentIntersectsRect
} from "./routeGeometry.js";
import {
  anchorFor,
  offsetForEndpointOrder,
  portFor,
  sideVector
} from "./routePorts.js";
import { getCachedRawRoutes, routeCacheKey, setCachedRawRoutes } from "./routeCache.js";
import { normalizeRouteStyle } from "./routeStyle.js";
import { estimatedLabelBox, withReadableLabel } from "./routeLabels.js";
import { edgeCorridors, freeSpaceCorridors } from "./routeCorridors.js";
import { createRouteCandidateFactory } from "./routeCandidateBuilders.js";
import { selectRouteCandidate } from "./routeStrategies.js";
import { CANVAS_INSET, ROUTE_COST_WEIGHTS, rectCenter } from "./routeConstants.js";

export { pathToSvgWithHops } from "./routeRendering.js";
export { distanceToRect, lineSamples, nearestSample } from "./routeGeometry.js";
export { anchorFor, sideVector } from "./routePorts.js";

export function routeIntersectsRect(route, rect, padding = 0) {
  if (route.sampleBounds && !rectsOverlap(route.sampleBounds, rect, padding)) return false;
  if (route.style === "orthogonal" && route.points) {
    for (let index = 0; index < route.points.length - 1; index += 1) {
      if (segmentIntersectsRect(route.points[index], route.points[index + 1], rect, padding)) return true;
    }
    return false;
  }
  return route.samples.some((point) =>
    point.x > rect.x - padding &&
    point.x < rect.x + rect.width + padding &&
    point.y > rect.y - padding &&
    point.y < rect.y + rect.height + padding
  );
}

function renderOrthogonalRoute(route, previousRoutes) {
  return withReadableLabel({ ...route, d: pathToSvgWithHops(route.points, previousRoutes), sampleBounds: boundsForPoints(route.samples), style: "orthogonal" });
}

function endpointSide(rect, point) {
  if (point.x === rect.x) return "left";
  if (point.x === rect.x + rect.width) return "right";
  if (point.y === rect.y) return "top";
  if (point.y === rect.y + rect.height) return "bottom";
  return "";
}

function sideEndpointKey(nodeId, side) {
  return `${nodeId}\u0000${side}`;
}

function sideNeedsPostSelectionCentering(side) {
  return side === "left" || side === "right" || side === "top" || side === "bottom";
}

function recenteredEndpointPoints(points, endpointIndex, rect, side) {
  return endpointOffsetPoints(points, endpointIndex, rect, side, 0);
}

function endpointOffsetPoints(points, endpointIndex, rect, side, rawOffset) {
  const nextPoints = points.map((point) => ({ ...point }));
  endpointIndex = endpointIndex === 0 ? 0 : nextPoints.length - 1;
  const oldAnchor = nextPoints[endpointIndex];
  const anchor = rawOffset === 0 ? anchorFor(rect, side) : portFor(rect, side, 18, rawOffset).anchor;
  nextPoints[endpointIndex] = anchor;
  const adjacentIndex = endpointIndex === 0 ? 1 : nextPoints.length - 2;
  if (rawOffset === 0 && nextPoints[adjacentIndex] && nextPoints.length > 2) {
    const elbowIndex = endpointIndex === 0 ? 2 : nextPoints.length - 3;
    if (side === "top" || side === "bottom") {
      nextPoints[adjacentIndex].x = anchor.x;
      if (nextPoints[elbowIndex]?.x === oldAnchor.x) nextPoints[elbowIndex].x = anchor.x;
    } else {
      nextPoints[adjacentIndex].y = anchor.y;
      if (nextPoints[elbowIndex]?.y === oldAnchor.y) nextPoints[elbowIndex].y = anchor.y;
    }
    return nextPoints;
  }
  if (nextPoints[adjacentIndex] && nextPoints.length > 2) {
    const elbowIndex = endpointIndex === 0 ? 2 : nextPoints.length - 3;
    const adjacent = nextPoints[adjacentIndex];
    const beforeAdjacent = endpointIndex === 0 ? nextPoints[elbowIndex] : nextPoints[elbowIndex];
    if (side === "top" || side === "bottom") {
      adjacent.x = anchor.x;
      const beforeBeforeAdjacent = endpointIndex === 0 ? nextPoints[elbowIndex + 1] : nextPoints[elbowIndex - 1];
      if (beforeBeforeAdjacent && beforeBeforeAdjacent.y === beforeAdjacent.y) {
        beforeAdjacent.x = adjacent.x;
      } else if (beforeAdjacent && beforeAdjacent.x !== adjacent.x && beforeAdjacent.y !== adjacent.y) {
        const elbow = { x: adjacent.x, y: beforeAdjacent.y };
        if (endpointIndex === 0) nextPoints.splice(adjacentIndex + 1, 0, elbow);
        else nextPoints.splice(adjacentIndex, 0, elbow);
      }
    } else {
      adjacent.y = anchor.y;
      const beforeBeforeAdjacent = endpointIndex === 0 ? nextPoints[elbowIndex + 1] : nextPoints[elbowIndex - 1];
      if (beforeBeforeAdjacent && beforeBeforeAdjacent.x === beforeAdjacent.x) {
        beforeAdjacent.y = adjacent.y;
      } else if (beforeAdjacent && beforeAdjacent.x !== adjacent.x && beforeAdjacent.y !== adjacent.y) {
        const elbow = { x: beforeAdjacent.x, y: adjacent.y };
        if (endpointIndex === 0) nextPoints.splice(adjacentIndex + 1, 0, elbow);
        else nextPoints.splice(adjacentIndex, 0, elbow);
      }
    }
  }
  return nextPoints;
}

function routeWithPoints(route, points, controls = route.controls) {
  const samples = route.style === "spline" && controls?.length === 2
    ? [points[0], ...sampleCubic(points[0], controls[0], controls[1], points.at(-1), 32)]
    : route.style === "straight"
      ? sampleLine(points[0], points.at(-1), 18)
      : lineSamples(points);
  const label = samples[Math.floor(samples.length / 2)] ?? points[Math.floor(points.length / 2)] ?? { x: 0, y: 0 };
  return {
    ...route,
    d: route.style === "spline" && controls?.length === 2
      ? `M ${points[0].x} ${points[0].y} C ${controls[0].x} ${controls[0].y} ${controls[1].x} ${controls[1].y} ${points.at(-1).x} ${points.at(-1).y}`
      : points.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x} ${point.y}`).join(" "),
    points,
    controls,
    samples,
    bends: bendCount(points),
    labelX: label.x,
    labelY: label.y
  };
}

function recenteredEndpointRoute(route, endpointIndex, rect, side) {
  const points = recenteredEndpointPoints(route.points, endpointIndex, rect, side);
  if (route.style !== "spline" || !route.controls?.length) return routeWithPoints(route, points);
  return routeWithPoints(route, points, route.controls.map((control) => ({ ...control })));
}

function offsetEndpointRoute(route, endpointIndex, rect, side, rawOffset) {
  const points = endpointOffsetPoints(route.points, endpointIndex, rect, side, rawOffset);
  if (route.style !== "spline" || !route.controls?.length) return routeWithPoints(route, points);
  return routeWithPoints(route, points, route.controls.map((control) => ({ ...control })));
}

function collapseAlignedOpposingSurfaceRoute(route, firstSide, lastSide) {
  const first = route.points[0];
  const last = route.points.at(-1);
  if ((firstSide === "left" || firstSide === "right") && (lastSide === "left" || lastSide === "right") && first.y === last.y) {
    return routeWithPoints(route, [first, last], route.controls);
  }
  if ((firstSide === "top" || firstSide === "bottom") && (lastSide === "top" || lastSide === "bottom") && first.x === last.x) {
    return routeWithPoints(route, [first, last], route.controls);
  }
  return route;
}

function alignedFixedPortRoute(route, relationship, input) {
  if (!route.points?.length) return route;
  const fromRect = input.nodeRects.get(relationship.from);
  const toRect = input.nodeRects.get(relationship.to);
  let points = route.points;
  if (fromRect?.fixedPorts && relationship.preferredStartSide && points[1]) {
    points = points.map((point) => ({ ...point }));
    if (relationship.preferredStartSide === "left" || relationship.preferredStartSide === "right") {
      points[1].y = points[0].y;
    } else {
      points[1].x = points[0].x;
    }
  }
  if (toRect?.fixedPorts && relationship.preferredEndSide && points.length > 1) {
    if (points === route.points) points = points.map((point) => ({ ...point }));
    const beforeEndIndex = points.length - 2;
    const endIndex = points.length - 1;
    if (relationship.preferredEndSide === "left" || relationship.preferredEndSide === "right") {
      points[beforeEndIndex].y = points[endIndex].y;
    } else {
      points[beforeEndIndex].x = points[endIndex].x;
    }
  }
  return points === route.points ? route : routeWithPoints(route, points, route.controls);
}

function axisAlignedSegments(route) {
  const segments = [];
  for (let index = 0; index < route.points.length - 1; index += 1) {
    const start = route.points[index];
    const end = route.points[index + 1];
    if (start.x === end.x) {
      segments.push({
        orientation: "vertical",
        x: start.x,
        min: Math.min(start.y, end.y),
        max: Math.max(start.y, end.y)
      });
    } else if (start.y === end.y) {
      segments.push({
        orientation: "horizontal",
        y: start.y,
        min: Math.min(start.x, end.x),
        max: Math.max(start.x, end.x)
      });
    }
  }
  return segments;
}

function sharedSegmentLength(left, right) {
  if (left.orientation !== right.orientation) return 0;
  if (left.orientation === "horizontal" && left.y !== right.y) return 0;
  if (left.orientation === "vertical" && left.x !== right.x) return 0;
  return Math.max(0, Math.min(left.max, right.max) - Math.max(left.min, right.min));
}

function sharedSegmentCount(route, otherRoutes) {
  let count = 0;
  for (const segment of axisAlignedSegments(route)) {
    for (const otherRoute of otherRoutes) {
      for (const otherSegment of axisAlignedSegments(otherRoute)) {
        if (sharedSegmentLength(segment, otherSegment) > 1) count += 1;
      }
    }
  }
  return count;
}

function recenteredWithoutNewSharedSegments(route, nextRoute, otherRoutes) {
  return sharedSegmentCount(nextRoute, otherRoutes) <= sharedSegmentCount(route, otherRoutes)
    ? nextRoute
    : route;
}

function endpointSpreadOffset(index, count, rect, side) {
  const sideLength = side === "left" || side === "right" ? rect.height : rect.width;
  return ((index + 1) / (count + 1) - 0.5) * sideLength;
}

function spreadSharedSideEndpoints(plannedRawRoutes, input) {
  const relationshipById = new Map(input.relationships.map((relationship) => [relationship.id, relationship]));
  const endpointGroups = new Map();
  for (const [relationshipId, route] of plannedRawRoutes) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || !route.points?.length) continue;
    if (relationship.relationshipType !== "flow") continue;
    const fromRect = input.nodeRects.get(relationship.from);
    const startSide = fromRect ? endpointSide(fromRect, route.points[0]) : "";
    if (fromRect && !fromRect.fixedPorts && sideNeedsPostSelectionCentering(startSide)) {
      const key = sideEndpointKey(relationship.from, startSide);
      endpointGroups.set(key, [...(endpointGroups.get(key) ?? []), { relationship, relationshipId, endpointIndex: 0, rect: fromRect, side: startSide }]);
    }
    const toRect = input.nodeRects.get(relationship.to);
    const endSide = toRect ? endpointSide(toRect, route.points.at(-1)) : "";
    if (toRect && !toRect.fixedPorts && sideNeedsPostSelectionCentering(endSide)) {
      const key = sideEndpointKey(relationship.to, endSide);
      endpointGroups.set(key, [...(endpointGroups.get(key) ?? []), { relationship, relationshipId, endpointIndex: route.points.length - 1, rect: toRect, side: endSide }]);
    }
  }

  const routeById = new Map(plannedRawRoutes);
  for (const endpoints of endpointGroups.values()) {
    if (endpoints.length <= 1) continue;
    if (!endpoints.some((endpoint) => endpoint.relationship.preferredStartSide || endpoint.relationship.preferredEndSide || endpoint.relationship.outcome)) continue;
    endpoints.forEach((endpoint, index) => {
      const route = routeById.get(endpoint.relationshipId);
      if (!route) return;
      const offsetRoute = offsetEndpointRoute(
        route,
        endpoint.endpointIndex,
        endpoint.rect,
        endpoint.side,
        endpointSpreadOffset(index, endpoints.length, endpoint.rect, endpoint.side)
      );
      const oppositeIndex = endpoint.endpointIndex === 0 ? offsetRoute.points.length - 1 : 0;
      const oppositeNodeId = endpoint.endpointIndex === 0 ? endpoint.relationship.to : endpoint.relationship.from;
      const oppositeRect = input.nodeRects.get(oppositeNodeId);
      const oppositeSide = oppositeRect ? endpointSide(oppositeRect, offsetRoute.points[oppositeIndex]) : "";
      const canAlignOpposite = (
        oppositeRect &&
        !oppositeRect.fixedPorts &&
        sideNeedsPostSelectionCentering(oppositeSide)
      );
      const alignedRoute = canAlignOpposite && (endpoint.side === "left" || endpoint.side === "right") && (oppositeSide === "left" || oppositeSide === "right")
        ? offsetEndpointRoute(
            offsetRoute,
            oppositeIndex,
            oppositeRect,
            oppositeSide,
            offsetRoute.points.at(endpoint.endpointIndex === 0 ? 0 : -1).y - rectCenter(oppositeRect).y
          )
        : canAlignOpposite && (endpoint.side === "top" || endpoint.side === "bottom") && (oppositeSide === "top" || oppositeSide === "bottom")
          ? offsetEndpointRoute(
              offsetRoute,
              oppositeIndex,
              oppositeRect,
              oppositeSide,
              offsetRoute.points.at(endpoint.endpointIndex === 0 ? 0 : -1).x - rectCenter(oppositeRect).x
            )
          : offsetRoute;
      const firstRect = input.nodeRects.get(endpoint.relationship.from);
      const lastRect = input.nodeRects.get(endpoint.relationship.to);
      const firstSide = firstRect ? endpointSide(firstRect, alignedRoute.points[0]) : "";
      const lastSide = lastRect ? endpointSide(lastRect, alignedRoute.points.at(-1)) : "";
      routeById.set(endpoint.relationshipId, collapseAlignedOpposingSurfaceRoute(alignedRoute, firstSide, lastSide));
    });
  }
  return plannedRawRoutes.map(([relationshipId]) => [relationshipId, routeById.get(relationshipId)]);
}

function recenterSingletonSideEndpoints(plannedRawRoutes, input) {
  const relationshipById = new Map(input.relationships.map((relationship) => [relationship.id, relationship]));
  const endpointCounts = new Map();
  const routeById = new Map(plannedRawRoutes);
  for (const [relationshipId, route] of plannedRawRoutes) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || !route.points?.length) continue;
    const fromRect = input.nodeRects.get(relationship.from);
    const toRect = input.nodeRects.get(relationship.to);
    const startSide = fromRect ? endpointSide(fromRect, route.points[0]) : "";
    const endSide = toRect ? endpointSide(toRect, route.points.at(-1)) : "";
    if (sideNeedsPostSelectionCentering(startSide)) endpointCounts.set(sideEndpointKey(relationship.from, startSide), (endpointCounts.get(sideEndpointKey(relationship.from, startSide)) ?? 0) + 1);
    if (sideNeedsPostSelectionCentering(endSide)) endpointCounts.set(sideEndpointKey(relationship.to, endSide), (endpointCounts.get(sideEndpointKey(relationship.to, endSide)) ?? 0) + 1);
  }

  return plannedRawRoutes.map(([relationshipId, route]) => {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || !route.points?.length) return [relationshipId, route];
    let points = route.points;
    const fromRect = input.nodeRects.get(relationship.from);
    const startSide = fromRect ? endpointSide(fromRect, points[0]) : "";
    if (fromRect && sideNeedsPostSelectionCentering(startSide) && endpointCounts.get(sideEndpointKey(relationship.from, startSide)) === 1) {
      const nextRoute = recenteredEndpointRoute(route, 0, fromRect, startSide);
      const otherRoutes = [...routeById].filter(([otherRelationshipId]) => otherRelationshipId !== relationshipId).map(([, otherRoute]) => otherRoute);
      route = recenteredWithoutNewSharedSegments(route, nextRoute, otherRoutes);
      points = route.points;
    }
    const toRect = input.nodeRects.get(relationship.to);
    const endSide = toRect ? endpointSide(toRect, points.at(-1)) : "";
    if (toRect && sideNeedsPostSelectionCentering(endSide) && endpointCounts.get(sideEndpointKey(relationship.to, endSide)) === 1) {
      const nextRoute = recenteredEndpointRoute(route, points.length - 1, toRect, endSide);
      const otherRoutes = [...routeById].filter(([otherRelationshipId]) => otherRelationshipId !== relationshipId).map(([, otherRoute]) => otherRoute);
      route = recenteredWithoutNewSharedSegments(route, nextRoute, otherRoutes);
      points = route.points;
    }
    routeById.set(relationshipId, route);
    return [relationshipId, alignedFixedPortRoute(route, relationship, input)];
  });
}

function routePlannerContext(input) {
  const visibleNodeIds = new Set(input.visibleNodeIds);
  const rectFor = (nodeId) => input.nodeRects.get(nodeId);
  const visibleRects = Array.from(visibleNodeIds).map(rectFor).filter(Boolean);
  const blockerCache = new Map();
  const stats = input.stats ?? null;
  const blockerRects = (fromId, toId) => {
    const key = `${fromId}\u0000${toId}`;
    const cached = blockerCache.get(key);
    if (cached) return cached;
    const blockers = Array.from(visibleNodeIds)
      .filter((nodeId) => nodeId !== fromId && nodeId !== toId)
      .map(rectFor)
      .filter(Boolean);
    blockerCache.set(key, blockers);
    return blockers;
  };

  const routeQualityFromSamples = (samples, label, fromId, toId, usedRoutes, relationship) => {
    const blockers = blockerRects(fromId, toId);
    const sampleBounds = boundsForPoints(samples);
    const sampleBlockers = blockers.filter((rect) => rectsOverlap(sampleBounds, rect, CANVAS_INSET.top));
    const labelBox = estimatedLabelBox(label, relationship);
    const labelBlockers = blockers.filter((rect) => {
      const labelPointBounds = { x: label.x, y: label.y, width: 0, height: 0 };
      return rectsOverlap(labelPointBounds, rect, 34) || (labelBox && rectsOverlap(labelBox, rect, 6));
    });
    const qualityCosts = {
      lengthCost: 0,
      boundaryCost: 0,
      nodeClearanceCost: 0,
      edgeProximityCost: 0,
      labelNodeClearanceCost: 0
    };
    for (let index = 0; index < samples.length - 1; index += 1) {
      qualityCosts.lengthCost += Math.hypot(samples[index + 1].x - samples[index].x, samples[index + 1].y - samples[index].y);
    }
    for (const point of samples) {
      if (point.y < CANVAS_INSET.top || point.x < 16 || point.x > input.canvasWidth - 16 || point.y > input.canvasHeight - 16) {
        qualityCosts.boundaryCost += ROUTE_COST_WEIGHTS.boundaryViolation;
      }
      for (const rect of sampleBlockers) {
        const distanceSquared = distanceToRectSquared(point, rect);
        if (distanceSquared < 900) {
          const distance = Math.sqrt(distanceSquared);
          if (distance < 14) qualityCosts.nodeClearanceCost += ROUTE_COST_WEIGHTS.nodeCollision;
          qualityCosts.nodeClearanceCost += (CANVAS_INSET.top - distance) * ROUTE_COST_WEIGHTS.nodeClearance;
        }
      }
      if (input.scoreEdgeProximity || input.style === "spline") {
        for (const usedRoute of usedRoutes) {
          for (let usedIndex = 0; usedIndex < usedRoute.length; usedIndex += 2) {
            const used = usedRoute[usedIndex];
            const distance = Math.hypot(point.x - used.x, point.y - used.y);
            if (distance < 36) qualityCosts.edgeProximityCost += 1800;
            if (distance < 20) qualityCosts.edgeProximityCost += 6200;
            if (distance < 10) qualityCosts.edgeProximityCost += 18000;
          }
        }
      }
    }
    for (const rect of labelBlockers) {
      if (distanceToRectSquared(label, rect) < 1156) qualityCosts.labelNodeClearanceCost += 24000;
      if (labelBox && rectsOverlap(labelBox, rect, 6)) qualityCosts.labelNodeClearanceCost += 60000;
    }
    return qualityCosts;
  };

  const collisionCount = (route, fromId, toId, padding = 0) => {
    let collisions = 0;
    for (const rect of blockerRects(fromId, toId)) {
      let collided = route.style === "spline"
        ? routeIntersectsRect(route, rect, padding)
        : false;
      if (!collided) {
        for (let index = 0; index < route.points.length - 1; index += 1) {
          if (segmentIntersectsRect(route.points[index], route.points[index + 1], rect, padding)) {
            collided = true;
            break;
          }
        }
      }
      if (collided) {
        collisions += 1;
      }
    }
    return collisions;
  };

  const diagramCorridors = freeSpaceCorridors(visibleRects, input.canvasWidth, input.canvasHeight);
  const routeCandidates = createRouteCandidateFactory({
    blockerRects,
    canvasHeight: input.canvasHeight,
    canvasWidth: input.canvasWidth,
    gridRouteMaxExpansions: input.gridRouteMaxExpansions,
    gridRouteMaxPoints: input.gridRouteMaxPoints,
    rectFor,
    routeQualityFromSamples,
    stats
  });

  const edgePath = (relationship, index, pairIndex, usedRoutes, previousRoutes, routeIndex, endpointOffsets, style = "orthogonal") => {
    const { from: fromId, to: toId } = relationship;
    const fromRect = rectFor(fromId);
    const toRect = rectFor(toId);
    const corridors = [
      ...edgeCorridors(fromRect, toRect, diagramCorridors),
      ...routeIndex.adjacentCorridors(fromRect, toRect)
    ];
    return selectRouteCandidate({
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
    });
  };

  return { edgePath };
}

export function routeEdges(input) {
  const usedRoutes = [];
  const rawRoutes = [];
  const routeIndex = createRouteIndex();
  const pairCounts = new Map();
  const endpointTotals = new Map();
  const endpointCounts = new Map();
  const style = normalizeRouteStyle(input.style);
  const cacheKey = routeCacheKey(input);
  const cachedRawRoutes = getCachedRawRoutes(cacheKey);
  const plannedRawRoutes = cachedRawRoutes ?? [];
  const planner = cachedRawRoutes ? null : routePlannerContext(input);

  if (!cachedRawRoutes) {
    for (const relationship of input.relationships) {
      if (!input.laneIndexByNode.has(relationship.from) || !input.laneIndexByNode.has(relationship.to)) {
        continue;
      }
      endpointTotals.set(relationship.from, (endpointTotals.get(relationship.from) ?? 0) + 1);
      endpointTotals.set(relationship.to, (endpointTotals.get(relationship.to) ?? 0) + 1);
    }

    input.relationships.forEach((relationship, index) => {
      if (!input.laneIndexByNode.has(relationship.from) || !input.laneIndexByNode.has(relationship.to)) {
        return;
      }

      const pairKey = [relationship.from, relationship.to].sort().join("<->");
      const pairIndex = pairCounts.get(pairKey) ?? 0;
      pairCounts.set(pairKey, pairIndex + 1);

      const fromEndpointCount = endpointCounts.get(relationship.from) ?? 0;
      const toEndpointCount = endpointCounts.get(relationship.to) ?? 0;
      endpointCounts.set(relationship.from, fromEndpointCount + 1);
      endpointCounts.set(relationship.to, toEndpointCount + 1);

      const route = planner.edgePath(
        relationship,
        index,
        pairIndex,
        usedRoutes,
        rawRoutes,
        routeIndex,
        {
          from: endpointTotals.get(relationship.from) === 1 ? 0 : offsetForEndpointOrder(fromEndpointCount),
          to: endpointTotals.get(relationship.to) === 1 ? 0 : offsetForEndpointOrder(toEndpointCount)
        },
        style
      );
      plannedRawRoutes.push([relationship.id, route]);
      usedRoutes.push(route.samples);
      rawRoutes.push(route);
      routeIndex.add(route, rawRoutes.length - 1);
    });
    setCachedRawRoutes(cacheKey, plannedRawRoutes);
  }

  const displayRawRoutes = spreadSharedSideEndpoints(recenterSingletonSideEndpoints(plannedRawRoutes, input), input);
  const routes = new Map();
  const renderedRoutes = [];
  for (const [relationshipId, rawRoute] of displayRawRoutes) {
    const route = style === "orthogonal" ? renderOrthogonalRoute(rawRoute, renderedRoutes) : rawRoute;
    routes.set(relationshipId, route);
    renderedRoutes.push(route);
  }
  return routes;
}
