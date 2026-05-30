import { MOUNT_COST, MIN_LEGIBLE_GAP, MOUNT_MAX_ITERS } from "./routeConstants.js";
import { surfaceCapacity } from "./routePorts.js";
import {
  endpointSide,
  axisAlignedSegments,
  sharedSegmentLength,
  sideNeedsPostSelectionCentering,
  routeCollidesWithNonEndpoints,
  routeHasEndpointTraversal,
  offsetEndpointRoute,
  endpointSpreadOffset
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

// Total wire length of a route (Euclidean over consecutive points; for orthogonal
// routes this is the Manhattan path length). Only backtracking doglegs add length —
// a monotonic orthogonal path between two points has fixed length.
function routeLength(route) {
  let total = 0;
  for (let i = 0; i < route.points.length - 1; i += 1) {
    total += Math.hypot(route.points[i + 1].x - route.points[i].x, route.points[i + 1].y - route.points[i].y);
  }
  return total;
}

const pointKey = (p) => `${p.x},${p.y}`;

// Count every visual intersection between two routes — X crossings AND T-junctions /
// touches (one route's corner or stub landing on the other's edge), which a strict
// straddle test misses. Excludes shared mounts (both routes terminating at the same
// node port — a legitimate convergence, not a crossing). Each distinct point counts once.
export function routeIntersections(routeA, routeB) {
  const segsA = axisAlignedSegments(routeA);
  const segsB = axisAlignedSegments(routeB);
  const terminalA = new Set([pointKey(routeA.points[0]), pointKey(routeA.points.at(-1))]);
  const terminalB = new Set([pointKey(routeB.points[0]), pointKey(routeB.points.at(-1))]);
  const points = new Set();
  for (const left of segsA) {
    for (const right of segsB) {
      if (left.orientation === right.orientation) continue;
      const h = left.orientation === "horizontal" ? left : right;
      const v = left.orientation === "horizontal" ? right : left;
      if (v.x >= h.min && v.x <= h.max && h.y >= v.min && h.y <= v.max) {
        const key = `${v.x},${h.y}`;
        if (terminalA.has(key) && terminalB.has(key)) continue; // shared mount
        points.add(key);
      }
    }
  }
  return points.size;
}

export function mountAssignmentCost(routeById, relationshipById, input) {
  let cost = 0;
  const routes = [...routeById.entries()];
  // tier 0: collisions; tier 4: bends
  for (const [id, route] of routes) {
    const rel = relationshipById.get(id);
    if (rel && routeCollidesWithNonEndpoints(route, rel, input)) cost += MOUNT_COST.collision;
    if (rel && routeHasEndpointTraversal(route, rel, input)) cost += MOUNT_COST.endpointTraversal;
    cost += (route.bends ?? 0) * MOUNT_COST.bend;
    cost += (route.repeatedCrossings ?? 0) * MOUNT_COST.repeatedCrossing;
    cost += (route.selfOverlappingSegments ?? 0) * MOUNT_COST.selfOverlap;
    cost += routeLength(route) * MOUNT_COST.length;            // tier 5 — prefer shorter wire
  }
  // tiers 2/3: pairwise crossings + shared segments
  for (let i = 0; i < routes.length; i += 1) {
    for (let j = i + 1; j < routes.length; j += 1) {
      cost += routeIntersections(routes[i][1], routes[j][1]) * MOUNT_COST.crossing;
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
    cost += surfaceSpacingCost(surface.positions, length);
  }
  return cost;
}

// positions: mount coordinates along the surface axis, expressed as distance
// from the surface start (0..length). Only legibility costs: a gap (between adjacent
// mounts or from a mount to a surface corner) below MIN_LEGIBLE_GAP is crowding and is
// penalized. Mounts that are unevenly placed but still legibly spaced are FREE — even
// spread is an aesthetic, not a legibility requirement, so it is not charged here.
export function surfaceSpacingCost(positions, length) {
  const sorted = [...positions].sort((a, b) => a - b);
  let cost = 0;
  const guards = [0, ...sorted, length];
  for (let i = 0; i < guards.length - 1; i += 1) {
    const gap = guards[i + 1] - guards[i];
    if (gap < MIN_LEGIBLE_GAP) cost += (MIN_LEGIBLE_GAP - gap) * MOUNT_COST.cramped;
  }
  return cost;
}

function isStraightFacing(route) {
  const a = route.points[0];
  const b = route.points.at(-1);
  return route.points.length === 2 && (a.x === b.x || a.y === b.y);
}

// target: { id, endpointIndex, side, rect }. delta: signed shift along the surface axis.
// Moves the target mount, then — if the edge was a straight facing line — co-shifts the
// partner end by the same delta so the edge stays straight instead of bending.
export function applyOffsetWithMatch(routeById, relationshipById, input, target, delta) {
  const route = routeById.get(target.id);
  const rel = relationshipById.get(target.id);
  const straightFacing = isStraightFacing(route);
  const axis = target.side === "left" || target.side === "right" ? "y" : "x";
  const center = axis === "y" ? target.rect.y + target.rect.height / 2 : target.rect.x + target.rect.width / 2;
  const point = target.endpointIndex === 0 ? route.points[0] : route.points.at(-1);
  let moved = offsetEndpointRoute(route, target.endpointIndex, target.rect, target.side, point[axis] - center + delta);
  routeById.set(target.id, moved);
  if (!straightFacing) return;
  // Matched movement: co-shift the partner end so the straight facing edge stays straight.
  const partnerIndex = target.endpointIndex === 0 ? moved.points.length - 1 : 0;
  const partnerNodeId = target.endpointIndex === 0 ? rel.to : rel.from;
  const partnerRect = input.nodeRects.get(partnerNodeId);
  if (!partnerRect) return;
  const partnerPoint = partnerIndex === 0 ? moved.points[0] : moved.points.at(-1);
  const partnerSide = endpointSide(partnerRect, partnerPoint);
  const partnerCenter = axis === "y" ? partnerRect.y + partnerRect.height / 2 : partnerRect.x + partnerRect.width / 2;
  moved = offsetEndpointRoute(moved, partnerIndex, partnerRect, partnerSide, partnerPoint[axis] - partnerCenter + delta);
  routeById.set(target.id, moved);
}

const SIDES = ["top", "right", "bottom", "left"];

// Deterministic deep clone of a routeById Map for trial/accept.
function snapshotRoutes(routeById) {
  return new Map([...routeById].map(([id, r]) => [id, { ...r, points: r.points.map((p) => ({ ...p })) }]));
}

// Group movable endpoints by surface, carrying the descriptors respread needs.
// Ordered by the opposite node's centre (stable, non-circular) so the spread is
// deterministic and crossing-minimal for the common facing case.
function surfaceEndpointGroups(routeById, relationshipById, input) {
  const groups = new Map(); // `${nodeId} ${side}` -> endpoint descriptors
  for (const [id, route] of routeById) {
    const rel = relationshipById.get(id);
    if (!rel || !route?.points?.length) continue;
    for (const [nodeId, endpointIndex, oppositeId] of [[rel.from, 0, rel.to], [rel.to, route.points.length - 1, rel.from]]) {
      const rect = input.nodeRects.get(nodeId);
      const point = endpointIndex === 0 ? route.points[0] : route.points.at(-1);
      const side = rect ? endpointSide(rect, point) : "";
      if (!rect || rect.fixedPorts || !sideNeedsPostSelectionCentering(side)) continue;
      const key = `${nodeId} ${side}`;
      if (!groups.has(key)) groups.set(key, []);
      const opp = input.nodeRects.get(oppositeId);
      const axis = side === "left" || side === "right" ? "y" : "x";
      const oppCentre = opp ? (axis === "y" ? opp.y + opp.height / 2 : opp.x + opp.width / 2) : 0;
      groups.get(key).push({ id, endpointIndex, rect, side, oppCentre, displayIndex: rel.displayIndex ?? 0 });
    }
  }
  return groups;
}

// Stages 1+2: re-spread each surface's mounts to their ideal evenly-spaced slots.
function respreadSurfaces(routeById, relationshipById, input) {
  for (const endpoints of surfaceEndpointGroups(routeById, relationshipById, input).values()) {
    if (endpoints.length < 2) continue;
    endpoints.sort((a, b) => a.oppCentre - b.oppCentre || a.displayIndex - b.displayIndex || a.id.localeCompare(b.id));
    endpoints.forEach((ep, index) => {
      const route = routeById.get(ep.id);
      const offset = endpointSpreadOffset(index, endpoints.length, ep.rect, ep.side);
      routeById.set(ep.id, offsetEndpointRoute(route, ep.endpointIndex, ep.rect, ep.side, offset));
    });
  }
}

// Item 2: try moving each endpoint to a different surface side, keeping a change
// only if it strictly lowers the global cost and does not collide. Re-spreads
// after each trial so the cost reflects the post-spread geometry.
function trySideMoves(routeById, relationshipById, input, buildRouteForSides) {
  if (!buildRouteForSides) return;
  const ids = [...routeById.keys()].sort();
  for (const id of ids) {
    const rel = relationshipById.get(id);
    const route = routeById.get(id);
    if (!rel || !route?.points?.length) continue;
    const fromRect = input.nodeRects.get(rel.from);
    const toRect = input.nodeRects.get(rel.to);
    if (!fromRect || !toRect) continue;
    const startSide = endpointSide(fromRect, route.points[0]);
    const endSide = endpointSide(toRect, route.points.at(-1));
    for (const candidateStart of SIDES) {
      for (const candidateEnd of SIDES) {
        if (candidateStart === startSide && candidateEnd === endSide) continue;
        const before = mountAssignmentCost(routeById, relationshipById, input);
        const saved = snapshotRoutes(routeById);
        const rebuilt = buildRouteForSides(rel, candidateStart, candidateEnd);
        if (!rebuilt || routeCollidesWithNonEndpoints(rebuilt, rel, input)) continue;
        routeById.set(id, rebuilt);
        respreadSurfaces(routeById, relationshipById, input);
        if (mountAssignmentCost(routeById, relationshipById, input) >= before) {
          for (const [savedId, savedRoute] of saved) routeById.set(savedId, savedRoute);
        }
      }
    }
  }
}

// Staged local search: per-surface respread + scored side moves, accepted only
// when the whole-diagram cost drops. The snapshot/accept guard makes the result
// a deterministic fixed point (idempotent on replan) regardless of cache state.
export function optimizeMountAssignments(routeById, relationshipById, input, options = {}) {
  const buildRouteForSides = options.buildRouteForSides ?? null;
  for (let iter = 0; iter < MOUNT_MAX_ITERS; iter += 1) {
    const before = mountAssignmentCost(routeById, relationshipById, input);
    const saved = snapshotRoutes(routeById);
    respreadSurfaces(routeById, relationshipById, input);
    trySideMoves(routeById, relationshipById, input, buildRouteForSides);
    if (mountAssignmentCost(routeById, relationshipById, input) >= before) {
      for (const [id, r] of saved) routeById.set(id, r);
      break;
    }
  }
}
