import { MOUNT_COST, MIN_LEGIBLE_GAP } from "./routeConstants.js";
import { surfaceCapacity } from "./routePorts.js";
import {
  endpointSide,
  crossingsBetween,
  axisAlignedSegments,
  sharedSegmentLength,
  sideNeedsPostSelectionCentering,
  routeCollidesWithNonEndpoints
} from "./routeEdges.js";

function movableEndpoints(routeById, relationshipById, input) {
  const out = [];
  for (const [id, route] of routeById) {
    const rel = relationshipById.get(id);
    if (!rel || !route?.points?.length) continue;
    for (const [nodeId, endpointIndex] of [[rel.from, 0], [rel.to, route.points.length - 1]]) {
      const rect = input.nodeRects.get(nodeId);
      const point = endpointIndex === 0 ? route.points[0] : route.points.at(-1);
      const side = rect ? endpointSide(rect, point) : "";
      if (!rect || rect.fixedPorts || !sideNeedsPostSelectionCentering(side)) continue;
      out.push({ id, endpointIndex, nodeId, side, rect, point });
    }
  }
  return out;
}

export function surfacesOf(routeById, relationshipById, input) {
  const surfaces = new Map(); // key `${nodeId} ${side}` -> { rect, side, positions[] }
  for (const ep of movableEndpoints(routeById, relationshipById, input)) {
    const key = `${ep.nodeId} ${ep.side}`;
    const axisStart = ep.side === "left" || ep.side === "right" ? ep.rect.y : ep.rect.x;
    const pos = (ep.side === "left" || ep.side === "right" ? ep.point.y : ep.point.x) - axisStart;
    if (!surfaces.has(key)) surfaces.set(key, { rect: ep.rect, side: ep.side, positions: [] });
    surfaces.get(key).positions.push(pos);
  }
  return surfaces;
}

export function mountAssignmentCost(routeById, relationshipById, input) {
  let cost = 0;
  const routes = [...routeById.entries()];
  // tier 0: collisions; tier 4: bends
  for (const [id, route] of routes) {
    const rel = relationshipById.get(id);
    if (rel && routeCollidesWithNonEndpoints(route, rel, input)) cost += MOUNT_COST.collision;
    cost += (route.bends ?? 0) * MOUNT_COST.bend;
  }
  // tiers 2/3: pairwise crossings + shared segments
  for (let i = 0; i < routes.length; i += 1) {
    for (let j = i + 1; j < routes.length; j += 1) {
      cost += crossingsBetween(routes[i][1], routes[j][1]) * MOUNT_COST.crossing;
      const segsA = axisAlignedSegments(routes[i][1]);
      const segsB = axisAlignedSegments(routes[j][1]);
      for (const l of segsA) for (const r of segsB) {
        const len = sharedSegmentLength(l, r);
        if (len > 1) cost += MOUNT_COST.sharedSegment + len * MOUNT_COST.sharedSegmentLength;
      }
    }
  }
  // tier 0 capacity + tier 5 spacing, per surface
  for (const surface of surfacesOf(routeById, relationshipById, input).values()) {
    const length = surface.side === "left" || surface.side === "right" ? surface.rect.height : surface.rect.width;
    const count = surface.positions.length;
    if (count > surfaceCapacity(surface.rect, surface.side)) cost += MOUNT_COST.overCapacity;
    cost += surfaceSpacingCost(surface.positions, length, count);
  }
  return cost;
}

// positions: mount coordinates along the surface axis, expressed as distance
// from the surface start (0..length). count: total mounts on the surface.
export function surfaceSpacingCost(positions, length, count) {
  const sorted = [...positions].sort((a, b) => a - b);
  let cost = 0;
  // Deviation from the ideal evenly-spread slots (mounts may run to corners).
  sorted.forEach((pos, index) => {
    const ideal = ((index + 1) / (count + 1)) * length;
    cost += Math.abs(pos - ideal) * MOUNT_COST.spacingDeviation;
  });
  // Steep sub-penalty when any adjacent gap (incl. surface ends) is sub-legible.
  const guards = [0, ...sorted, length];
  for (let i = 0; i < guards.length - 1; i += 1) {
    const gap = guards[i + 1] - guards[i];
    if (gap < MIN_LEGIBLE_GAP) cost += (MIN_LEGIBLE_GAP - gap) * MOUNT_COST.cramped;
  }
  return cost;
}
