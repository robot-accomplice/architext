import { normalizeRouteStyle } from "./routeStyle.js";

const RAW_ROUTE_CACHE_LIMIT = 12;
const rawRouteCache = new Map();

function mapEntries(map) {
  return Array.from(map.entries()).sort(([left], [right]) => String(left).localeCompare(String(right)));
}

export function routeCacheKey(input) {
  return JSON.stringify({
    style: normalizeRouteStyle(input.style),
    relationships: input.relationships.map((relationship) => ({
      id: relationship.id,
      from: relationship.from,
      to: relationship.to,
      label: relationship.label,
      relationshipType: relationship.relationshipType,
      kind: relationship.kind,
      returnOf: relationship.returnOf,
      outcome: relationship.outcome,
      stepId: relationship.stepId,
      flowId: relationship.flowId,
      preferredStartSide: relationship.preferredStartSide,
      preferredEndSide: relationship.preferredEndSide
    })),
    visibleNodeIds: Array.from(input.visibleNodeIds).sort(),
    nodeRects: mapEntries(input.nodeRects),
    laneIndexByNode: mapEntries(input.laneIndexByNode),
    rowIndexByNode: mapEntries(input.rowIndexByNode),
    canvasWidth: input.canvasWidth,
    canvasHeight: input.canvasHeight,
    marginY: input.marginY,
    gridRouteMaxPoints: input.gridRouteMaxPoints,
    gridRouteMaxExpansions: input.gridRouteMaxExpansions,
    scoreEdgeProximity: Boolean(input.scoreEdgeProximity)
  });
}

export function getCachedRawRoutes(key) {
  const cached = rawRouteCache.get(key);
  if (!cached) return null;
  rawRouteCache.delete(key);
  rawRouteCache.set(key, cached);
  return cached;
}

export function setCachedRawRoutes(key, value) {
  rawRouteCache.set(key, value);
  while (rawRouteCache.size > RAW_ROUTE_CACHE_LIMIT) {
    rawRouteCache.delete(rawRouteCache.keys().next().value);
  }
}
