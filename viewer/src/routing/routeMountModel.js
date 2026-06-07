import {
  MOUNT_COST, MIN_LEGIBLE_GAP, MOUNT_MAX_ITERS, rectCenter,
  RECIPROCAL_PARALLEL_OFFSET, BRIDGE_MOUNT_OFFSET, BRIDGE_GUTTER_CLEARANCE, BRIDGE_LANE_GAP, BRIDGE_MAX_LANES
} from "./routeConstants.js";
import { surfaceCapacity } from "./routePorts.js";
import { deriveRouteIntent, semanticSurfaceOptions } from "./routeIntent.js";
import { shallowJogCount } from "./routeGeometry.js";
import {
  endpointSide,
  axisAlignedSegments,
  sharedSegmentLength,
  sideNeedsPostSelectionCentering,
  routeCollidesWithNonEndpoints,
  routeHasEndpointTraversal,
  offsetEndpointRoute,
  endpointSpreadOffset,
  routeWithPoints,
  offsetOrthogonalPolyline
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
// routes this is the Manhattan path length).
function routeLength(route) {
  let total = 0;
  for (let i = 0; i < route.points.length - 1; i += 1) {
    total += Math.hypot(route.points[i + 1].x - route.points[i].x, route.points[i + 1].y - route.points[i].y);
  }
  return total;
}

// The shortest possible wire between two nodes: the Manhattan gap between their bounding
// boxes (0 on an axis where they overlap). This is the irreducible distance the layout
// imposes — no routing choice can beat it.
function nodeGapLength(fromRect, toRect) {
  if (!fromRect || !toRect) return 0;
  const gapX = Math.max(0, fromRect.x - (toRect.x + toRect.width), toRect.x - (fromRect.x + fromRect.width));
  const gapY = Math.max(0, fromRect.y - (toRect.y + toRect.height), toRect.y - (fromRect.y + fromRect.height));
  return gapX + gapY;
}

// The AVOIDABLE length of a route: how far it overshoots the shortest possible wire between
// its two nodes. A direct/monotonic route is 0; a detour or wrap-around mount is charged its
// overshoot. Base distance (fixed by the layout) is never charged — a necessary long edge is
// not a defect. This replaces raw wire length so the objective does not assume every pixel of
// length is an equal unit of cost.
export function excessLength(route, fromRect, toRect) {
  if (!route?.points?.length) return 0;
  return Math.max(0, routeLength(route) - nodeGapLength(fromRect, toRect));
}

const pointKey = (p) => `${p.x},${p.y}`;
const SIDE_NORMAL = { top: { x: 0, y: -1 }, bottom: { x: 0, y: 1 }, left: { x: -1, y: 0 }, right: { x: 1, y: 0 } };

// Count segments that travel AGAINST the overall from->to direction — a route that
// doubles back (a dogleg). Mirrors the existing monotonicBacktrack notion but counts
// reversing segments as discrete events for the tiered objective.
export function doglegCount(route, fromRect, toRect) {
  if (!fromRect || !toRect || !route?.points?.length) return 0;
  const from = rectCenter(fromRect);
  const to = rectCenter(toRect);
  const xDir = Math.sign(to.x - from.x);
  const yDir = Math.sign(to.y - from.y);
  let count = 0;
  for (let i = 0; i < route.points.length - 1; i += 1) {
    const dx = route.points[i + 1].x - route.points[i].x;
    const dy = route.points[i + 1].y - route.points[i].y;
    if (xDir !== 0 && Math.sign(dx) === -xDir) count += 1;
    if (yDir !== 0 && Math.sign(dy) === -yDir) count += 1;
  }
  return count;
}

// Count endpoints that mount on a node side facing AWAY from the opposite node (the
// outward side normal points opposite the direction to the partner). Such mounts make
// an edge leave the wrong way and read as less intentional.
export function intentMismatchCount(route, relationship, input) {
  if (!route?.points?.length) return 0;
  let count = 0;
  for (const [nodeId, endpointIndex, oppositeId] of [[relationship.from, 0, relationship.to], [relationship.to, route.points.length - 1, relationship.from]]) {
    const rect = input.nodeRects.get(nodeId);
    const opp = input.nodeRects.get(oppositeId);
    if (!rect || !opp) continue;
    const point = endpointIndex === 0 ? route.points[0] : route.points.at(-1);
    const normal = SIDE_NORMAL[endpointSide(rect, point)];
    if (!normal) continue;
    const c = rectCenter(rect);
    const o = rectCenter(opp);
    if (normal.x * (o.x - c.x) + normal.y * (o.y - c.y) < 0) count += 1;
  }
  return count;
}

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

// Count only TRUE X-crossings between two routes — segments that strictly straddle each
// other (interior intersection), NOT T-junctions or touches. The optimizer scores moves by
// this stricter count: a T-junction is not a visual crossing, and chasing one would make the
// optimizer mangle an otherwise clean route to remove a phantom. (The live four-pass still
// counts T-junctions; only the mount objective uses this strict view.)
export function strictCrossingCount(routeA, routeB) {
  const segsA = axisAlignedSegments(routeA);
  const segsB = axisAlignedSegments(routeB);
  let count = 0;
  for (const left of segsA) {
    for (const right of segsB) {
      if (left.orientation === right.orientation) continue;
      const h = left.orientation === "horizontal" ? left : right;
      const v = left.orientation === "horizontal" ? right : left;
      if (v.x > h.min && v.x < h.max && h.y > v.min && h.y < v.max) count += 1;
    }
  }
  return count;
}

// The whole-diagram objective. Every contributing factor is reduced to a raw, unweighted
// magnitude in mountCostFactors, then weighted UNIFORMLY here — one tunable weight per factor
// in MOUNT_COST, no factor special-cased. Costs are NOT assumed equal: each factor carries its
// own weight, and none is a raw wire-length (the `length` factor is avoidable detour only).
export function mountAssignmentCost(routeById, relationshipById, input) {
  return weightedMountCost(mountCostFactors(routeById, relationshipById, input));
}

// Weighted sum of a pre-measured factor breakdown. Split out from mountAssignmentCost so a caller
// that already has the factor vector (e.g. to also read factors.crossing) can score it without
// recomputing the O(E^2) measurement.
function weightedMountCost(factors) {
  let cost = 0;
  for (const factor of Object.keys(factors)) cost += (MOUNT_COST[factor] ?? 0) * factors[factor];
  return cost;
}

// The objective is a single WEIGHTED SUM (`mountAssignmentCost`), not a tiered/lexicographic
// vector. One knob set: each factor's weight in MOUNT_COST is the only lever, so tuning is
// predictable — raising a weight cannot be silently overridden by a tier boundary, and there is
// no second variable (tier order) interacting with the weights. Priorities are expressed by the
// MAGNITUDE of the weights (collision ≫ overlap ≫ shared-segment ≫ crossing ≫ … ≫ length), and a
// high-but-finite crossing weight is what lets a route accept a crossing rather than wrap the
// whole diagram to avoid it (a crossing is worth ~crossing/length px of detour). Every optimizer
// guard below compares `mountAssignmentCost` directly.

// Raw magnitude of each cost factor across the diagram, before weighting. Keeping the
// measurement (here) separate from the weighting (in mountAssignmentCost) is what lets any
// factor be re-weighted in one place, and makes the per-factor breakdown inspectable.
export function mountCostFactors(routeById, relationshipById, input) {
  const factors = {
    collision: 0, endpointTraversal: 0, repeatedCrossing: 0, selfOverlap: 0,
    sharedSegment: 0, sharedSegmentLength: 0, perimeterFallback: 0, crossing: 0, monotonicBacktrack: 0,
    bend: 0, dogleg: 0, shallowJog: 0, cramped: 0, intentMismatch: 0, length: 0, overCapacity: 0
  };
  const routes = [...routeById.entries()];
  for (const [id, route] of routes) {
    const rel = relationshipById.get(id);
    if (rel && routeCollidesWithNonEndpoints(route, rel, input)) factors.collision += 1;
    if (rel && routeHasEndpointTraversal(route, rel, input)) factors.endpointTraversal += 1;
    factors.bend += route.bends ?? 0;
    factors.shallowJog += shallowJogCount(route.points);                       // the small stair-steps doglegCount misses — always avoidable by aligning the mounts
    factors.repeatedCrossing += route.repeatedCrossings ?? 0;
    factors.selfOverlap += route.selfOverlappingSegments ?? 0;
    if ((route.qualityCosts?.perimeterFallbackCost ?? 0) > 0) factors.perimeterFallback += 1;
    if ((route.qualityCosts?.monotonicBacktrackCost ?? 0) > 0) factors.monotonicBacktrack += 1;
    if (rel) {
      const fromRect = input.nodeRects.get(rel.from);
      const toRect = input.nodeRects.get(rel.to);
      factors.length += excessLength(route, fromRect, toRect);                 // avoidable detour, not raw length
      factors.dogleg += doglegCount(route, fromRect, toRect);
      factors.intentMismatch += intentMismatchCount(route, rel, input);
    }
  }
  for (let i = 0; i < routes.length; i += 1) {
    for (let j = i + 1; j < routes.length; j += 1) {
      factors.crossing += strictCrossingCount(routes[i][1], routes[j][1]);
      const segsA = axisAlignedSegments(routes[i][1]);
      const segsB = axisAlignedSegments(routes[j][1]);
      for (const l of segsA) for (const r of segsB) {
        const len = sharedSegmentLength(l, r);
        if (len > 1) { factors.sharedSegment += 1; factors.sharedSegmentLength += len; }
      }
    }
  }
  for (const surface of surfacesOf(routeById, relationshipById, input).values()) {
    const length = surface.side === "left" || surface.side === "right" ? surface.rect.height : surface.rect.width;
    factors.overCapacity += Math.max(0, surface.positions.length - surfaceCapacity(surface.rect, surface.side));
    factors.cramped += surfaceCrampedUnits(surface.positions, length);
  }
  return factors;
}

// Raw crowding magnitude of a surface, UNWEIGHTED: the total amount by which gaps fall
// below MIN_LEGIBLE_GAP (between adjacent mounts, or from a mount to a surface corner).
// positions are mount coordinates along the surface axis (0..length). Legibly-spaced but
// uneven mounts are FREE — even spread is an aesthetic, not a legibility requirement.
export function surfaceCrampedUnits(positions, length) {
  const sorted = [...positions].sort((a, b) => a - b);
  let units = 0;
  const guards = [0, ...sorted, length];
  for (let i = 0; i < guards.length - 1; i += 1) {
    const gap = guards[i + 1] - guards[i];
    if (gap < MIN_LEGIBLE_GAP) units += MIN_LEGIBLE_GAP - gap;
  }
  return units;
}

// Weighted crowding cost of a surface (the cramped factor applied). Retained for callers
// and tests that score a single surface in isolation.
export function surfaceSpacingCost(positions, length) {
  return surfaceCrampedUnits(positions, length) * MOUNT_COST.cramped;
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
    // Only flow edges carry the directional/semantic intent the mount cost model reasons
    // about; structural (dependency) relationships are laid out by their own four-pass path
    // and re-homing them by this objective mis-optimizes them. Leave them as routed.
    if (rel.relationshipType !== "flow") continue;
    // Respect the same pins the four-pass cascade honors: an endpoint fixed to a port or
    // steered to a preferred side (decision branches, explicit entry/exit sides) must not
    // be re-homed by the optimizer.
    if (rel.preferredStartSide || rel.preferredEndSide) continue;
    const fromRect = input.nodeRects.get(rel.from);
    const toRect = input.nodeRects.get(rel.to);
    if (!fromRect || !toRect) continue;
    if (fromRect.fixedPorts || toRect.fixedPorts) continue;
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

// Edges the four tuned passes left crowded enough to reconsider: an endpoint sits on
// an over-capacity surface, OR the edge is one half of a reciprocal pair (A->B and
// B->A) whose route crosses another edge. The crossing trigger is restricted to
// reciprocal pairs on purpose — a single-direction edge that crosses a sibling fan or
// escapes under a blocker is doing so intentionally (obstacle-aware), so it is left
// alone; only a reciprocal pair forced through a crowded fan, which should run parallel
// on an open gutter instead, is reconsidered. Returned in deterministic id order so the
// tuned layout of every other edge is left untouched.
function reliefCandidateIds(routeById, relationshipById, input) {
  const overCapacitySurfaces = new Set();
  for (const [key, surface] of surfacesOf(routeById, relationshipById, input)) {
    if (surface.positions.length > surfaceCapacity(surface.rect, surface.side)) overCapacitySurfaces.add(key);
  }
  const directed = new Set();
  for (const rel of relationshipById.values()) {
    if (routeById.has(rel.id)) directed.add(`${rel.from}\0${rel.to}`);
  }
  const routes = [...routeById.entries()];
  const crossing = new Set();
  for (let i = 0; i < routes.length; i += 1) {
    for (let j = i + 1; j < routes.length; j += 1) {
      if (routeIntersections(routes[i][1], routes[j][1]) > 0) {
        crossing.add(routes[i][0]);
        crossing.add(routes[j][0]);
      }
    }
  }
  const ids = [];
  for (const [id, route] of routeById) {
    const rel = relationshipById.get(id);
    if (!rel || !route?.points?.length) continue;
    const fromRect = input.nodeRects.get(rel.from);
    const toRect = input.nodeRects.get(rel.to);
    const startSide = fromRect ? endpointSide(fromRect, route.points[0]) : "";
    const endSide = toRect ? endpointSide(toRect, route.points.at(-1)) : "";
    const onOverCapacity = overCapacitySurfaces.has(`${rel.from} ${startSide}`) || overCapacitySurfaces.has(`${rel.to} ${endSide}`);
    const reciprocalCrossing = crossing.has(id) && directed.has(`${rel.to}\0${rel.from}`);
    if (onOverCapacity || reciprocalCrossing) ids.push(id);
  }
  return ids.sort();
}

// Whether a side faces toward the partner node at all (its outward normal has a
// positive component toward the partner) — as opposed to a perpendicular or away
// escape. A facing mount on an uncrowded surface is intentional (e.g. a fan of edges
// into one hub side) and must not be disturbed; a perpendicular escape (dot 0) is fair
// game for re-homing onto a cleaner side.
function sideFacesPartner(side, rect, partnerRect) {
  const center = rectCenter(rect);
  const partner = rectCenter(partnerRect);
  const normal = SIDE_NORMAL[side];
  return normal.x * (partner.x - center.x) + normal.y * (partner.y - center.y) > 0;
}

// The side whose outward normal points MOST directly at the partner — the side an edge
// most naturally mounts on. Distinct from sideFacesPartner: a side can face the partner
// (positive dot) without being the ideal one (e.g. a node's right side weakly faces a
// partner that is mostly below it; its ideal side is bottom).
function idealFacingSide(rect, partnerRect) {
  const center = rectCenter(rect);
  const partner = rectCenter(partnerRect);
  const dx = partner.x - center.x;
  const dy = partner.y - center.y;
  let best = SIDES[0];
  let bestDot = -Infinity;
  for (const side of SIDES) {
    const normal = SIDE_NORMAL[side];
    const dot = normal.x * dx + normal.y * dy;
    if (dot > bestDot) {
      bestDot = dot;
      best = side;
    }
  }
  return best;
}

// Every reciprocal pair (A->B and B->A between the same two nodes) that has a route.
// Returned as [idA, idB] with idA < idB, in deterministic order.
function reciprocalPairs(routeById, relationshipById) {
  const byPair = new Map();
  for (const rel of relationshipById.values()) {
    if (!routeById.has(rel.id)) continue;
    const key = [rel.from, rel.to].sort().join(" ");
    if (!byPair.has(key)) byPair.set(key, []);
    byPair.get(key).push(rel.id);
  }
  const pairs = [];
  for (const ids of byPair.values()) {
    if (ids.length === 2) pairs.push([...ids].sort());
  }
  return pairs.sort((a, b) => a[0].localeCompare(b[0]));
}

// Reciprocal pairs where at least one half crosses another edge — the pair was forced
// through a crowded fan and should instead run parallel on a clean gutter.
function reciprocalCrossingPairs(routeById, relationshipById, input) {
  const routes = [...routeById.entries()];
  const crossing = new Set();
  for (let i = 0; i < routes.length; i += 1) {
    for (let j = i + 1; j < routes.length; j += 1) {
      if (routeIntersections(routes[i][1], routes[j][1]) > 0) {
        crossing.add(routes[i][0]);
        crossing.add(routes[j][0]);
      }
    }
  }
  return reciprocalPairs(routeById, relationshipById).filter(([a, b]) => crossing.has(a) || crossing.has(b));
}

// Surgical relief, run AFTER the four tuned passes. They occasionally leave a node side
// over capacity, or route a reciprocal pair through a crowded fan (crossings) because
// its facing side was blocked, forcing a perpendicular escape. Two phases, both gated by
// the whole-diagram cost guard so the pass can only improve or no-op (worst case it
// validates the prior passes); the snapshot/accept guard keeps it a deterministic fixed
// point. Phase 1 moves each crowded reciprocal pair JOINTLY onto a shared escape gutter
// (both halves on one node side; the crossing-reduction swap that runs after this pass then
// orders their mounts so they nest) — moving them one at a time would split the pair. Phase 2
// spills the marginal endpoint of any surface
// still over capacity onto a cleaner side. Neither phase moves an endpoint off a side that
// faces its partner while that surface is within capacity, so a legible facing fan stays put.
export function relieveCrowdedSurfaces(routeById, relationshipById, input, buildRouteForSides) {
  if (!buildRouteForSides) return;
  const cost = () => mountAssignmentCost(routeById, relationshipById, input);
  const restore = (saved) => { for (const [id, route] of saved) routeById.set(id, route); };
  const surfaceOverCapacity = (nodeId, side) => {
    const rect = input.nodeRects.get(nodeId);
    if (!rect) return false;
    const surface = surfacesOf(routeById, relationshipById, input).get(`${nodeId} ${side}`);
    return surface ? surface.positions.length > surfaceCapacity(rect, side) : false;
  };
  // Freeze an endpoint that is on its IDEAL facing side (always — even over capacity, so a
  // facing pair like a hub and the service beside it is relieved by spilling escapes, not by
  // doglegging the facing return), OR that merely faces its partner while its surface is within
  // capacity (so a legible fan keeps its members on the shared facing side). A weakly-facing
  // escape on an over-capacity surface stays movable so the surface can actually be relieved.
  const frozenForEndpoint = (rect, partnerRect, side, nodeId) =>
    side === idealFacingSide(rect, partnerRect) ||
    (sideFacesPartner(side, rect, partnerRect) && !surfaceOverCapacity(nodeId, side));

  const movedPairs = [];
  // Phase 1: joint reciprocal-pair moves onto a shared escape gutter.
  for (const [idA, idB] of reciprocalCrossingPairs(routeById, relationshipById, input)) {
    const relA = relationshipById.get(idA);
    const relB = relationshipById.get(idB);
    const routeA = routeById.get(idA);
    if (!relA || !relB || !routeA?.points?.length) continue;
    const fromRect = input.nodeRects.get(relA.from);
    const toRect = input.nodeRects.get(relA.to);
    if (!fromRect || !toRect) continue;
    const startSide = endpointSide(fromRect, routeA.points[0]);
    const endSide = endpointSide(toRect, routeA.points.at(-1));
    const startFrozen = frozenForEndpoint(fromRect, toRect, startSide, relA.from);
    const endFrozen = frozenForEndpoint(toRect, fromRect, endSide, relA.to);
    for (const side of SIDES) {
      if (startFrozen && side !== startSide) continue;
      if (endFrozen && side !== endSide) continue;
      if (side === startSide && side === endSide) continue;
      const before = cost();
      const saved = snapshotRoutes(routeById);
      const newA = buildRouteForSides(relA, side, side, routeById);
      if (!newA || routeCollidesWithNonEndpoints(newA, relA, input)) { restore(saved); continue; }
      routeById.set(idA, newA);
      const newB = buildRouteForSides(relB, side, side, routeById);
      if (!newB || routeCollidesWithNonEndpoints(newB, relB, input)) { restore(saved); continue; }
      routeById.set(idB, newB);
      if (cost() < before) { movedPairs.push([idA, idB]); break; }
      restore(saved);
    }
  }

  // Phase 2: spill the marginal endpoint of any surface still over capacity.
  let spilled = false;
  for (const id of reliefCandidateIds(routeById, relationshipById, input)) {
    const rel = relationshipById.get(id);
    const route = routeById.get(id);
    if (!rel || !route?.points?.length) continue;
    const fromRect = input.nodeRects.get(rel.from);
    const toRect = input.nodeRects.get(rel.to);
    if (!fromRect || !toRect) continue;
    const startSide = endpointSide(fromRect, route.points[0]);
    const endSide = endpointSide(toRect, route.points.at(-1));
    if (!surfaceOverCapacity(rel.from, startSide) && !surfaceOverCapacity(rel.to, endSide)) continue;
    const startFrozen = frozenForEndpoint(fromRect, toRect, startSide, rel.from);
    const endFrozen = frozenForEndpoint(toRect, fromRect, endSide, rel.to);
    for (const candidateStart of SIDES) {
      if (startFrozen && candidateStart !== startSide) continue;
      for (const candidateEnd of SIDES) {
        if (endFrozen && candidateEnd !== endSide) continue;
        if (candidateStart === startSide && candidateEnd === endSide) continue;
        const before = cost();
        const saved = snapshotRoutes(routeById);
        const rebuilt = buildRouteForSides(rel, candidateStart, candidateEnd, routeById);
        if (!rebuilt || routeCollidesWithNonEndpoints(rebuilt, rel, input)) continue;
        routeById.set(id, rebuilt);
        if (cost() < before) { spilled = true; break; }
        restore(saved);
      }
    }
  }
  // pairs: the reciprocal pairs Phase 1 relocated onto a shared gutter — the caller
  // re-parallels ONLY these so untouched pairs keep their existing lane separation.
  // anyMoved: whether relief changed any route at all, so the caller knows to re-spread
  // surfaces (a relief rebuild can land an endpoint beside its neighbours, not in the open
  // slot) and re-run the crossing-reduction swap.
  return { pairs: movedPairs, anyMoved: movedPairs.length > 0 || spilled };
}

// Geometric construction of a crossing-free reciprocal bridge on the top or bottom gutter:
// the request runs on an inner lane, the return nests on an outer lane, and their mounts are
// offset to opposite sides of each node's surface centre so the return arc ENCLOSES the request
// arc (0 within-pair crossings by construction — no grid search, no proximity scoring). Used by
// reciprocalParallelMoves when both ends of the pair should escape onto a shared gutter.
export function buildReciprocalGutterBridge(requestRel, returnRel, requestRoute, returnRoute, input, side, gutterClearance = BRIDGE_GUTTER_CLEARANCE) {
  const ra = input.nodeRects.get(requestRel.from);
  const rb = input.nodeRects.get(requestRel.to);
  if (!ra || !rb) return null;
  const PAD = 8; // keep mounts off the surface corners (matches portFor inset)
  const surfYa = side === "top" ? ra.y : ra.y + ra.height;
  const surfYb = side === "top" ? rb.y : rb.y + rb.height;
  const aCx = ra.x + ra.width / 2;
  const bCx = rb.x + rb.width / 2;
  const towardB = Math.sign(bCx - aCx) || 1;
  const clampX = (rect, x) => Math.max(rect.x + PAD, Math.min(rect.x + rect.width - PAD, x));
  // request mounts inner (toward the partner); return mounts outer (away from it).
  const reqAx = clampX(ra, aCx + towardB * BRIDGE_MOUNT_OFFSET);
  const retAx = clampX(ra, aCx - towardB * BRIDGE_MOUNT_OFFSET);
  const reqBx = clampX(rb, bCx - towardB * BRIDGE_MOUNT_OFFSET);
  const retBx = clampX(rb, bCx + towardB * BRIDGE_MOUNT_OFFSET);
  const edge = side === "top"
    ? Math.min(ra.y, rb.y) - gutterClearance
    : Math.max(ra.y + ra.height, rb.y + rb.height) + gutterClearance;
  const laneReq = edge;
  const laneRet = side === "top" ? edge - BRIDGE_LANE_GAP : edge + BRIDGE_LANE_GAP;
  const request = routeWithPoints(requestRoute, [
    { x: reqAx, y: surfYa }, { x: reqAx, y: laneReq }, { x: reqBx, y: laneReq }, { x: reqBx, y: surfYb }
  ]);
  const ret = routeWithPoints(returnRoute, [
    { x: retBx, y: surfYb }, { x: retBx, y: laneRet }, { x: retAx, y: laneRet }, { x: retAx, y: surfYa }
  ]);
  return { request, return: ret };
}

// Rebuild a request route as a MONOTONIC staircase on its current mount surfaces: keep both mount
// points, connect them with an orthogonal path that turns at a SHARED elbow (mid-column for a
// facing horizontal pair, mid-row for a vertical pair, the departure-respecting corner for a mixed
// L). A monotonic staircase has ZERO doglegs by construction — every leg runs with the from->to
// direction, never against it — so when the planner left a congestion overshoot, this offers the
// clean shape. reciprocalParallelMoves mirrors it for the return; the stage's cost+crossing+facing
// guard keeps it only if it actually lowers cost without adding a crossing or losing facing.
export function buildMonotonicStaircase(requestRoute, startSide, endSide, elbow) {
  const pA = requestRoute.points[0];
  const pB = requestRoute.points.at(-1);
  const horiz = (side) => side === "left" || side === "right";
  let points;
  if (horiz(startSide) && horiz(endSide)) {
    points = pA.y === pB.y ? [pA, pB] : [pA, { x: elbow, y: pA.y }, { x: elbow, y: pB.y }, pB];
  } else if (!horiz(startSide) && !horiz(endSide)) {
    points = pA.x === pB.x ? [pA, pB] : [pA, { x: pA.x, y: elbow }, { x: pB.x, y: elbow }, pB];
  } else {
    // Mixed L: leave pA perpendicular to its side, arrive pB perpendicular to its side.
    const corner = horiz(startSide) ? { x: pB.x, y: pA.y } : { x: pA.x, y: pB.y };
    points = [pA, corner, pB];
  }
  return routeWithPoints(requestRoute, points);
}

// Clear elbow coordinates along `axis` between lo..hi: the centres of the gutters between visible
// nodes whose PERPENDICULAR span overlaps the staircase's band, so the turning leg threads an open
// channel instead of crossing a node column (the naive midpoint usually lands on one). Up to `max`
// candidates, nearest-the-midpoint first; the caller's collision/crossing guard makes the final call.
function clearElbows(input, axis, lo, hi, bandLo, bandHi, max = 4) {
  const a = Math.min(lo, hi);
  const b = Math.max(lo, hi);
  const occupied = [];
  for (const id of input.visibleNodeIds ?? []) {
    const r = input.nodeRects.get(id);
    if (!r) continue;
    const spanLo = axis === "x" ? r.y : r.x;
    const spanHi = axis === "x" ? r.y + r.height : r.x + r.width;
    if (spanHi <= bandLo || spanLo >= bandHi) continue;
    occupied.push(axis === "x" ? [r.x, r.x + r.width] : [r.y, r.y + r.height]);
  }
  occupied.sort((p, q) => p[0] - q[0]);
  const gutters = [];
  let cursor = a;
  for (const [s, e] of occupied) {
    if (s > cursor) gutters.push((cursor + Math.min(s, b)) / 2);
    cursor = Math.max(cursor, e);
    if (cursor >= b) break;
  }
  if (cursor < b) gutters.push((cursor + b) / 2);
  const mid = (a + b) / 2;
  return gutters.filter((g) => g > a && g < b).sort((p, q) => Math.abs(p - mid) - Math.abs(q - mid)).slice(0, max);
}

// How many of a route's two endpoints mount OFF the surface the routeDiagnostics intent model
// expects to face the partner. Uses the SAME lane/row-aware deriveRouteIntent as the diagnostic
// (NOT pure geometric facing), so the reciprocal stage's facing guard measures exactly what the
// "non-facing" finding measures. See the guard for why this is needed beyond the cost model's
// intentMismatch factor.
function routeNonFacingCount(route, rel, input) {
  const fromRect = input.nodeRects.get(rel.from);
  const toRect = input.nodeRects.get(rel.to);
  if (!fromRect || !toRect) return 0;
  const intent = deriveRouteIntent({
    relationship: rel,
    fromRect,
    toRect,
    fromLaneIndex: input.laneIndexByNode?.get(rel.from),
    toLaneIndex: input.laneIndexByNode?.get(rel.to),
    fromRowIndex: input.rowIndexByNode?.get(rel.from),
    toRowIndex: input.rowIndexByNode?.get(rel.to)
  });
  let count = 0;
  if (endpointSide(fromRect, route.points[0]) !== intent.expectedSourceSide) count += 1;
  if (endpointSide(toRect, route.points.at(-1)) !== intent.expectedTargetSide) count += 1;
  return count;
}

// Lane/row-aware off-facing endpoints that are NOT a justified semantic escape — i.e. exactly the
// endpoints the routeDiagnostics "non-facing-*-surface" finding (and the mount-audit) flag. A mount on
// the expected facing surface, OR on a semantic escape surface the blocked-corridor model allows
// (e.g. both ends escaping to a side gutter around a same-column blocker), is NOT a defect.
// routeNonFacingCount (used as the reciprocal GUARD) counts raw side!=expected; that over-counts as an
// optimization TARGET and would "correct" legitimate gutter escapes back through their blocker, so this
// pass measures the justified-escape-aware count the diagnostic actually emits.
export function routeUnjustifiedNonFacing(route, rel, input) {
  const fromRect = input.nodeRects.get(rel.from);
  const toRect = input.nodeRects.get(rel.to);
  if (!fromRect || !toRect) return 0;
  const intent = deriveRouteIntent({
    relationship: rel, fromRect, toRect,
    fromLaneIndex: input.laneIndexByNode?.get(rel.from),
    toLaneIndex: input.laneIndexByNode?.get(rel.to),
    fromRowIndex: input.rowIndexByNode?.get(rel.from),
    toRowIndex: input.rowIndexByNode?.get(rel.to)
  });
  const sourceSide = endpointSide(fromRect, route.points[0]);
  const targetSide = endpointSide(toRect, route.points.at(-1));
  if (sourceSide === intent.expectedSourceSide && targetSide === intent.expectedTargetSide) return 0;
  const blockerRects = [...(input.visibleNodeIds ?? [])]
    .filter((nodeId) => nodeId !== rel.from && nodeId !== rel.to)
    .map((nodeId) => input.nodeRects.get(nodeId))
    .filter(Boolean);
  const options = semanticSurfaceOptions({
    expectedSides: { source: intent.expectedSourceSide, target: intent.expectedTargetSide },
    relationship: rel, fromRect, toRect, blockerRects,
    canvasWidth: input.canvasWidth, canvasHeight: input.canvasHeight
  });
  let count = 0;
  if (sourceSide !== intent.expectedSourceSide && !options.source.has(sourceSide)) count += 1;
  if (targetSide !== intent.expectedTargetSide && !options.target.has(targetSide)) count += 1;
  return count;
}

// Sum of unjustified off-facing endpoints across every flow edge — the quantity tryIntentFacingMoves
// drives down (matches the routeDiagnostics finding the mount-audit ranks).
function totalNonFacing(routeById, relationshipById, input) {
  let total = 0;
  for (const [id, route] of routeById) {
    const rel = relationshipById.get(id);
    if (!rel || rel.relationshipType !== "flow" || !route?.points?.length) continue;
    total += routeUnjustifiedNonFacing(route, rel, input);
  }
  return total;
}

// True if NO hard factor regressed. bend/length are polish (paid for by the facing gain below);
// intentMismatch (the GEOMETRY far-edge-wrap term in the weighted objective) is excluded on purpose:
// it can disagree with the lane/row-aware facing model, and when they disagree the lane-aware model
// wins (it is what the diagnostic and the maintainer read). Every other factor — collisions, crossings,
// all dogleg variants, crowding, over-capacity, shared segments — is a hard floor this pass must not raise.
function noHardFactorWorsening(before, after) {
  for (const key of Object.keys(after)) {
    if (key === "bend" || key === "length" || key === "intentMismatch") continue;
    if (after[key] > before[key]) return false;
  }
  return true;
}

// Polish objective for the facing pass: one facing correction (intentMismatch weight 1500) outranks a
// single added bend (900), so a move may straighten intent at the cost of one polish bend but never the
// reverse; length breaks ties toward shorter wire.
function facingPolishCost(nonFacing, factors) {
  return nonFacing * MOUNT_COST.intentMismatch + factors.bend * MOUNT_COST.bend + factors.length * MOUNT_COST.length;
}

// Decoupled final polish pass: pull a single mount that sits OFF its lane/row-aware facing surface onto
// a facing (or justified-escape) surface, but ONLY when the move lowers the facing polish cost and
// worsens NO hard factor. Facing is deliberately NOT a term in mountAssignmentCost (the weighted search
// is byte-identical to before this pass existed): in the objective it diverts the greedy crossing search
// into worse local optima — it over-piles mounts onto "correct" surfaces past capacity (the documented
// regression). Running LAST, from the crossing-optimal layout, with a hard non-worsening guard, the pass
// provably cannot raise crossings or over-capacity; it only claims the safe facing corrections the
// per-edge and reciprocal stages leave on the table. One edge at a time: cases that need a coordinated
// multi-edge re-layout to face correctly are left as routed, by design (facing yields to legibility).
function tryIntentFacingMoves(routeById, relationshipById, input, buildRouteForSides) {
  if (!buildRouteForSides) return;
  for (const id of [...routeById.keys()].sort()) {
    const rel = relationshipById.get(id);
    const route = routeById.get(id);
    if (!rel || !route?.points?.length || rel.relationshipType !== "flow") continue;
    if (rel.preferredStartSide || rel.preferredEndSide) continue;
    const fromRect = input.nodeRects.get(rel.from);
    const toRect = input.nodeRects.get(rel.to);
    if (!fromRect || !toRect || fromRect.fixedPorts || toRect.fixedPorts) continue;
    if (routeUnjustifiedNonFacing(route, rel, input) === 0) continue;
    const startSide = endpointSide(fromRect, route.points[0]);
    const endSide = endpointSide(toRect, route.points.at(-1));
    const beforeFactors = mountCostFactors(routeById, relationshipById, input);
    const beforePolish = facingPolishCost(totalNonFacing(routeById, relationshipById, input), beforeFactors);
    const saved = snapshotRoutes(routeById);
    let bestPolish = beforePolish;
    let bestState = null;
    for (const candStart of SIDES) {
      for (const candEnd of SIDES) {
        if (candStart === startSide && candEnd === endSide) continue;
        const rebuilt = buildRouteForSides(rel, candStart, candEnd, routeById);
        if (!rebuilt?.points?.length || routeCollidesWithNonEndpoints(rebuilt, rel, input)) continue;
        routeById.set(id, rebuilt);
        respreadSurfaces(routeById, relationshipById, input);
        const factors = mountCostFactors(routeById, relationshipById, input);
        const polish = facingPolishCost(totalNonFacing(routeById, relationshipById, input), factors);
        if (polish < bestPolish && noHardFactorWorsening(beforeFactors, factors)) {
          bestPolish = polish;
          bestState = snapshotRoutes(routeById);
        }
        for (const [sid, sr] of saved) routeById.set(sid, sr);
      }
    }
    if (bestState) for (const [sid, sr] of bestState) routeById.set(sid, sr);
  }
}

// Per-node-pair stage: for each reciprocal flow pair (A->B and B->A), JOINTLY re-home the
// request and return so the return mirrors its request instead of switchbacking. The per-edge
// trySideMoves CANNOT reach this: mirroring the return alone wraps it around the partner's far
// side, so the two ends must co-move. Candidates are COUPLED (request+return together): a
// fixed-gap parallel mirror (cheapest — the return runs as a constant offset of the request) plus
// top/bottom gutter bridges at increasing lane heights (for pairs whose request also wants to
// vacate a congested surface). Cost-guarded with the SAME weighted-sum objective as the rest of
// the optimizer: the cheapest coupled move that lowers total cost wins. A pair with nothing to
// gain is left untouched — worst case the stage validates prior stages.
export function reciprocalParallelMoves(routeById, relationshipById, input, buildRouteForSides = null) {
  const byNodePair = new Map();
  for (const rel of relationshipById.values()) {
    if (rel.relationshipType !== "flow" || !routeById.has(rel.id)) continue;
    const key = [rel.from, rel.to].sort().join(" ");
    if (!byNodePair.has(key)) byNodePair.set(key, []);
    byNodePair.get(key).push(rel);
  }
  for (const group of byNodePair.values()) {
    if (group.length < 2) continue;
    // Decompose the node pair into reciprocal sub-pairs (each A->B with its B->A) and mirror EACH:
    // a pair joined by more than one round trip (memory<->sqlite carries query/return AND
    // ingest/return) must get every return run parallel to its request, not just the lone-pair case.
    // Pair by displayIndex adjacency, NOT by a direction-keyed map: a map keyed on `${from} ${to}`
    // collapses the two same-direction requests (query AND curate are both memory->sqlite) onto one
    // key, which mis-pairs each request with the OTHER round trip's return. Match each request, in
    // flow order, with the nearest not-yet-paired return that follows it (a return is the opposite
    // direction with a later displayIndex), so query/return and curate/return stay distinct pairs.
    const sorted = [...group].sort((a, b) => (a.displayIndex ?? 0) - (b.displayIndex ?? 0));
    const paired = new Set();
    for (const request of sorted) {
      if (paired.has(request.id)) continue;
      const ret = sorted.find((o) =>
        !paired.has(o.id) && o.id !== request.id &&
        o.from === request.to && o.to === request.from &&
        (o.displayIndex ?? 0) >= (request.displayIndex ?? 0)
      );
      if (!ret) continue;
      paired.add(request.id);
      paired.add(ret.id);
    const savedRequest = routeById.get(request.id);
    const savedReturn = routeById.get(ret.id);
    if (!savedRequest?.points?.length || !savedReturn?.points?.length) continue;

    // Cheapest first: mirror the return as a fixed-gap parallel offset of the request.
    const reversed = [...savedRequest.points].reverse();
    const coupled = [];
    for (const delta of [RECIPROCAL_PARALLEL_OFFSET, -RECIPROCAL_PARALLEL_OFFSET]) {
      coupled.push({ request: savedRequest, return: routeWithPoints(savedReturn, offsetOrthogonalPolyline(reversed, delta)) });
    }
    // Shared-corner staircase: rebuild the request as a monotonic (dogleg-free) staircase on its
    // current surfaces and mirror it. The plain mirror above preserves whatever dogleg the request
    // already has; this offers the clean shape when the planner left a congestion overshoot. Guarded
    // below — kept only if cheaper with no added crossing and no lost facing.
    const ras = input.nodeRects.get(request.from);
    const rbs = input.nodeRects.get(request.to);
    if (ras && rbs) {
      const pA = savedRequest.points[0];
      const pB = savedRequest.points.at(-1);
      const startSide = endpointSide(ras, pA);
      const endSide = endpointSide(rbs, pB);
      const horiz = (side) => side === "left" || side === "right";
      // Sweep clear gutter elbows for a facing Z (turning leg threads an open channel); the mixed-L
      // case has a single departure-respecting corner (elbow ignored).
      let elbows;
      if (horiz(startSide) && horiz(endSide)) elbows = clearElbows(input, "x", pA.x, pB.x, Math.min(pA.y, pB.y), Math.max(pA.y, pB.y));
      else if (!horiz(startSide) && !horiz(endSide)) elbows = clearElbows(input, "y", pA.y, pB.y, Math.min(pA.x, pB.x), Math.max(pA.x, pB.x));
      else elbows = [0];
      for (const elbow of elbows) {
        const staircase = buildMonotonicStaircase(savedRequest, startSide, endSide, elbow);
        const reversedStair = [...staircase.points].reverse();
        for (const delta of [RECIPROCAL_PARALLEL_OFFSET, -RECIPROCAL_PARALLEL_OFFSET]) {
          coupled.push({ request: staircase, return: routeWithPoints(savedReturn, offsetOrthogonalPolyline(reversedStair, delta)) });
        }
      }
    }
    // Then gutter bridges at progressively higher lanes (a low lane crosses the congested band just
    // above/below the row; a higher one clears it). All arithmetic — no grid search.
    const ra = input.nodeRects.get(request.from);
    const rb = input.nodeRects.get(request.to);
    const laneStep = MIN_LEGIBLE_GAP * 2;
    if (ra && rb) {
      for (const side of ["top", "bottom"]) {
        const headroom = side === "top"
          ? Math.min(ra.y, rb.y) - MIN_LEGIBLE_GAP
          : (input.canvasHeight ?? Infinity) - Math.max(ra.y + ra.height, rb.y + rb.height) - MIN_LEGIBLE_GAP;
        for (let lane = 0; lane < BRIDGE_MAX_LANES; lane += 1) {
          const clearance = BRIDGE_GUTTER_CLEARANCE + lane * laneStep + BRIDGE_LANE_GAP;
          if (clearance > headroom) break;
          const bridge = buildReciprocalGutterBridge(request, ret, savedRequest, savedReturn, input, side, clearance);
          if (bridge) coupled.push(bridge);
        }
      }
    }
    // Coupled perpendicular-escape: when the request leaves its mounted surface and immediately
    // turns to travel perpendicular (a dogleg at the mount, because the facing corridor is blocked
    // so the route escapes up/down/around), re-home the request onto the surface it escapes TOWARD
    // and mirror the return to follow it. trySideMoves can't reach this per-edge: moving the request
    // alone drops its escape leg onto the return's lane (a shared segment the cost guard rejects),
    // so the return must co-move onto a parallel offset lane. Rebuilt around the current routes;
    // the guards below keep it only if it lowers cost without adding a crossing or losing facing.
    if (buildRouteForSides && ra && rb) {
      const reqStart = endpointSide(ra, savedRequest.points[0]);
      const reqEnd = endpointSide(rb, savedRequest.points.at(-1));
      // Vary BOTH the request's start AND end side — a blocked request often mounts the wrong
      // surface at both ends, so re-homing only the source leaves the target dogleg in place. The
      // weighted-sum + crossing-non-increase + facing guards below keep only a strict improvement.
      for (const candStart of SIDES) {
        for (const candEnd of SIDES) {
          if (candStart === reqStart && candEnd === reqEnd) continue;
          const rebuiltRequest = buildRouteForSides(request, candStart, candEnd, routeById);
          if (!rebuiltRequest?.points?.length) continue;
          const reversedRebuilt = [...rebuiltRequest.points].reverse();
          for (const delta of [RECIPROCAL_PARALLEL_OFFSET, -RECIPROCAL_PARALLEL_OFFSET]) {
            coupled.push({ request: rebuiltRequest, return: routeWithPoints(savedReturn, offsetOrthogonalPolyline(reversedRebuilt, delta)) });
          }
        }
      }
    }

    const beforeFactors = mountCostFactors(routeById, relationshipById, input);
    const before = weightedMountCost(beforeFactors);
    const savedNonFacing = routeNonFacingCount(savedRequest, request, input) + routeNonFacingCount(savedReturn, ret, input);
    let bestCost = before;
    let bestRequest = savedRequest;
    let bestReturn = savedReturn;
    for (const candidate of coupled) {
      if (routeCollidesWithNonEndpoints(candidate.request, request, input)) continue;
      if (routeCollidesWithNonEndpoints(candidate.return, ret, input)) continue;
      routeById.set(request.id, candidate.request);
      routeById.set(ret.id, candidate.return);
      const factors = mountCostFactors(routeById, relationshipById, input);
      routeById.set(request.id, savedRequest);
      routeById.set(ret.id, savedReturn);
      const cost = weightedMountCost(factors);
      // Keep the cheapest coupled move that lowers total cost.
      if (cost >= bestCost) continue;
      // Never let this legibility stage ADD a crossing. The global objective ranks doglegs above
      // crossings (dogleg 3300 > crossing 3000), so a mirror that trades one dogleg for a crossing
      // still lowers cost — fine as a global tradeoff, but wrong here: straightening a return must
      // not make the diagram cross more. Scoped to this stage; the global weighting is unchanged.
      if (factors.crossing > beforeFactors.crossing) continue;
      // Facing guard. The weighted-sum objective is blind to a return that leaves the surface
      // FACING its partner for a PERPENDICULAR one: its intentMismatch factor only counts a full
      // far-edge wrap, not a perpendicular mount, so a coupled move can shave crowding/bends by
      // pulling an endpoint off its facing surface — a legibility regression (the routeDiagnostics
      // "non-facing" finding) the cost can't see. Refuse any move that raises the pair's off-facing
      // endpoint count UNLESS it strictly removes a crossing, the one trade where leaving the
      // facing surface earns its keep (the witness: a return mirrors its request to kill a crossing).
      const candidateNonFacing = routeNonFacingCount(candidate.request, request, input) + routeNonFacingCount(candidate.return, ret, input);
      if (candidateNonFacing > savedNonFacing && factors.crossing >= beforeFactors.crossing) continue;
      bestCost = cost;
      bestRequest = candidate.request;
      bestReturn = candidate.return;
    }
    routeById.set(request.id, bestRequest);
    routeById.set(ret.id, bestReturn);
    }
  }
}

// Staged local search: per-surface respread + scored side moves + per-node-pair reciprocal
// coordination, accepted only when the whole-diagram cost drops. The snapshot/accept guard makes
// the result a deterministic fixed point (idempotent on replan) regardless of cache state.
export function optimizeMountAssignments(routeById, relationshipById, input, options = {}) {
  const buildRouteForSides = options.buildRouteForSides ?? null;
  const debug = typeof process !== "undefined" && process.env.MOUNT_DEBUG === "cost";
  const entryFactors = debug ? mountCostFactors(routeById, relationshipById, input) : null;
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
  // Per-node-pair reciprocal coordination runs once after the per-edge sweep converges: it co-moves
  // a reciprocal pair the per-edge moves can't reach (a return that should mirror its request but
  // switchbacks). Its own cost guard keeps it from regressing.
  reciprocalParallelMoves(routeById, relationshipById, input, buildRouteForSides);
  // Final decoupled polish: claim the safe single-edge facing corrections the search left behind. Runs
  // after the crossing-optimal layout is fixed and is hard-guarded against worsening any structural
  // factor, so it cannot raise crossings/over-capacity — it only moves a mount onto its facing surface
  // when that is free of cost in everything but bends/length.
  tryIntentFacingMoves(routeById, relationshipById, input, buildRouteForSides);
  if (debug && entryFactors) {
    const exitFactors = mountCostFactors(routeById, relationshipById, input);
    const diff = {};
    for (const k of Object.keys(entryFactors)) {
      if (entryFactors[k] !== exitFactors[k]) diff[k] = `${entryFactors[k]}->${exitFactors[k]} (w${MOUNT_COST[k] ?? 0})`;
    }
    if (Object.keys(diff).length) console.error(`[mount-cost ${input.relationships?.[0]?.flowId ?? "?"}] ${JSON.stringify(diff)}`);
  }
}
