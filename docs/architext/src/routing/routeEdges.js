import { pathToSvgWithHops } from "./routeRendering.js";
import { createRouteIndex } from "./routeIndex.js";
import {
  boundsForPoints,
  distanceToRect,
  distanceToRectSquared,
  lineSamples,
  nearestSample,
  rectsOverlap,
  segmentIntersectsRect
} from "./routeGeometry.js";
import {
  anchorFor,
  offsetForEndpointOrder,
  sideVector
} from "./routePorts.js";
import { getCachedRawRoutes, routeCacheKey, setCachedRawRoutes } from "./routeCache.js";
import { normalizeRouteStyle } from "./routeStyle.js";
import { estimatedLabelBox, withReadableLabel } from "./routeLabels.js";
import { edgeCorridors, freeSpaceCorridors } from "./routeCorridors.js";
import { createRouteCandidateFactory } from "./routeCandidateBuilders.js";
import { selectRouteCandidate } from "./routeStrategies.js";

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
    const sampleBlockers = blockers.filter((rect) => rectsOverlap(sampleBounds, rect, 30));
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
      if (point.y < 30 || point.x < 16 || point.x > input.canvasWidth - 16 || point.y > input.canvasHeight - 16) {
        qualityCosts.boundaryCost += 14000;
      }
      for (const rect of sampleBlockers) {
        const distanceSquared = distanceToRectSquared(point, rect);
        if (distanceSquared < 900) {
          const distance = Math.sqrt(distanceSquared);
          if (distance < 14) qualityCosts.nodeClearanceCost += 12000;
          qualityCosts.nodeClearanceCost += (30 - distance) * 120;
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
  const endpointCounts = new Map();
  const style = normalizeRouteStyle(input.style);
  const cacheKey = routeCacheKey(input);
  const cachedRawRoutes = getCachedRawRoutes(cacheKey);
  const plannedRawRoutes = cachedRawRoutes ?? [];
  const planner = cachedRawRoutes ? null : routePlannerContext(input);

  if (!cachedRawRoutes) {
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
          from: offsetForEndpointOrder(fromEndpointCount),
          to: offsetForEndpointOrder(toEndpointCount)
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

  const routes = new Map();
  const renderedRoutes = [];
  for (const [relationshipId, rawRoute] of plannedRawRoutes) {
    const route = style === "orthogonal" ? renderOrthogonalRoute(rawRoute, renderedRoutes) : rawRoute;
    routes.set(relationshipId, route);
    renderedRoutes.push(route);
  }
  return routes;
}
