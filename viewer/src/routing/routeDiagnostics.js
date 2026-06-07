import { segmentIntersectsRect } from "./routeGeometry.js";
import { anchorFor, surfaceCapacity } from "./routePorts.js";
import { deriveRouteIntent, semanticSurfaceOptions } from "./routeIntent.js";
import { crossingsBetween } from "./routeEdges.js";
import { reciprocalPairsByAdjacency } from "./routeReciprocal.js";
import { MIN_LEGIBLE_GAP } from "./routeConstants.js";

const POINT_EPSILON = 1;
// Two parallel segments are "merged" wherever they run closer than the same legibility gap mount
// points use — a parallel run gets the SAME buffer as a mount (MIN_LEGIBLE_GAP). This is the
// invariant: the space between two parallel lines, AT ANY POINT along their length, must be >=
// the floor. So there is deliberately NO minimum-span threshold — a sub-floor stretch counts no
// matter how short. A previous version required a 72px (then 24px) overlap, which silently missed
// short parallelism that arises AFTER a 90-degree bend (two lines turn and briefly run alongside).
// Any positive overlap below the floor is a violation; a single shared corner point (overlap 0) is
// not, since it has no length. Verified: the router already holds every corpus run at >= the floor,
// so this is a 0-violation guard, not a tuning knob.
const CLOSE_SEGMENT_DISTANCE = MIN_LEGIBLE_GAP; // px; parallel buffer floor, same as mount spacing
// Gutter lane-order detection: two routes share a face's gutter only if their perpendicular
// offsets differ by more than LANE_OFFSET_EPSILON (otherwise they are the same lane).
const LANE_OFFSET_EPSILON = 2;

export function sideForPoint(rect, point) {
  if (!rect || !point) return "";
  if (Math.abs(point.x - rect.x) <= POINT_EPSILON) return "left";
  if (Math.abs(point.x - (rect.x + rect.width)) <= POINT_EPSILON) return "right";
  if (Math.abs(point.y - rect.y) <= POINT_EPSILON) return "top";
  if (Math.abs(point.y - (rect.y + rect.height)) <= POINT_EPSILON) return "bottom";
  return "";
}

function sideOffset(rect, side, point) {
  const center = {
    x: rect.x + rect.width / 2,
    y: rect.y + rect.height / 2
  };
  if (side === "left" || side === "right") return point.y - center.y;
  if (side === "top" || side === "bottom") return point.x - center.x;
  return 0;
}

function endpointKey(nodeId, side) {
  return `${nodeId}\u0000${side}`;
}

function relationshipById(relationships) {
  return new Map(relationships.map((relationship) => [relationship.id, relationship]));
}

function endpointRecords(plan, relationships) {
  const byRelationship = relationshipById(relationships);
  const records = [];
  for (const [relationshipId, route] of plan.routes) {
    const relationship = byRelationship.get(relationshipId);
    if (!relationship || !route.points?.length) continue;
    const sourceRect = plan.nodeRects.get(relationship.from);
    const targetRect = plan.nodeRects.get(relationship.to);
    if (!sourceRect || !targetRect) continue;
    records.push({
      relationshipId,
      endpoint: "source",
      nodeId: relationship.from,
      rect: sourceRect,
      side: sideForPoint(sourceRect, route.points[0]),
      point: route.points[0]
    });
    records.push({
      relationshipId,
      endpoint: "target",
      nodeId: relationship.to,
      rect: targetRect,
      side: sideForPoint(targetRect, route.points.at(-1)),
      point: route.points.at(-1)
    });
  }
  return records;
}

function endpointCounts(records) {
  const counts = new Map();
  for (const record of records) {
    if (!record.side) continue;
    const key = endpointKey(record.nodeId, record.side);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

function axisAlignedSegments(route) {
  const segments = [];
  for (let index = 0; index < (route.points?.length ?? 0) - 1; index += 1) {
    const start = route.points[index];
    const end = route.points[index + 1];
    if (start.x === end.x) {
      segments.push({
        orientation: "vertical",
        line: start.x,
        min: Math.min(start.y, end.y),
        max: Math.max(start.y, end.y)
      });
    } else if (start.y === end.y) {
      segments.push({
        orientation: "horizontal",
        line: start.y,
        min: Math.min(start.x, end.x),
        max: Math.max(start.x, end.x)
      });
    }
  }
  return segments;
}

function segmentOverlap(left, right) {
  return Math.min(left.max, right.max) - Math.max(left.min, right.min);
}

function closeParallelRunCount(routes) {
  const routeEntries = [...routes];
  let count = 0;
  for (let leftIndex = 0; leftIndex < routeEntries.length; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < routeEntries.length; rightIndex += 1) {
      for (const left of axisAlignedSegments(routeEntries[leftIndex][1])) {
        for (const right of axisAlignedSegments(routeEntries[rightIndex][1])) {
          if (left.orientation !== right.orientation) continue;
          const overlap = segmentOverlap(left, right);
          if (overlap <= 0) continue; // need a real parallel stretch, not a single shared corner point
          // Strict <: a run sitting AT the legibility floor is compliant (mounts may sit exactly
          // MIN_LEGIBLE_GAP apart too); only a run tighter than the floor is a merge. No span gate —
          // a sub-floor stretch counts at any length (e.g. a brief alignment after a 90-degree bend).
          if (Math.abs(left.line - right.line) < CLOSE_SEGMENT_DISTANCE) count += 1;
        }
      }
    }
  }
  return count;
}

function hopCount(route) {
  return (route.d?.match(/\bQ\b/g) ?? []).length;
}

function routeFinding(code, message, details = {}) {
  return { code, message, ...details };
}

function expectedSideAtCapacity(endpoint, expectedSide, counts) {
  if (!endpoint?.rect || !expectedSide) return false;
  return (counts.get(endpointKey(endpoint.nodeId, expectedSide)) ?? 0) >= surfaceCapacity(endpoint.rect, expectedSide);
}

function straightExpectedPathBlocked(plan, relationship, expectedSourceSide, expectedTargetSide) {
  const sourceRect = plan.nodeRects.get(relationship.from);
  const targetRect = plan.nodeRects.get(relationship.to);
  if (!sourceRect || !targetRect) return false;
  const source = anchorFor(sourceRect, expectedSourceSide);
  const target = anchorFor(targetRect, expectedTargetSide);
  if (source.x !== target.x && source.y !== target.y) return false;
  for (const nodeId of plan.visibleNodeIds) {
    if (nodeId === relationship.from || nodeId === relationship.to) continue;
    const rect = plan.nodeRects.get(nodeId);
    if (rect && segmentIntersectsRect(source, target, rect, 0)) return true;
  }
  return false;
}

function constrainedSurfaceDeviation(plan, relationship, endpoint, expectedSide, expectedSourceSide, expectedTargetSide, counts) {
  if (expectedSideAtCapacity(endpoint, expectedSide, counts)) {
    return routeFinding("constrained-expected-surface-saturated", "Expected surface is already at capacity.", {
      endpoint: endpoint.endpoint,
      expected: expectedSide
    });
  }
  if (straightExpectedPathBlocked(plan, relationship, expectedSourceSide, expectedTargetSide)) {
    return routeFinding("constrained-expected-path-blocked", "Expected straight-facing path is blocked by intervening nodes.", {
      endpoint: endpoint.endpoint,
      expected: expectedSide
    });
  }
  return null;
}

function semanticSurfaceDeviationConstraint(plan, relationship, sourceRect, targetRect, intent, sourceSide, targetSide) {
  const blockerRects = [...plan.visibleNodeIds]
    .filter((nodeId) => nodeId !== relationship.from && nodeId !== relationship.to)
    .map((nodeId) => plan.nodeRects.get(nodeId))
    .filter(Boolean);
  const options = semanticSurfaceOptions({
    expectedSides: {
      source: intent.expectedSourceSide,
      target: intent.expectedTargetSide
    },
    relationship,
    fromRect: sourceRect,
    toRect: targetRect,
    blockerRects,
    canvasWidth: plan.canvasWidth,
    canvasHeight: plan.canvasHeight
  });
  return {
    source: options.source.has(sourceSide)
      ? routeFinding("constrained-primary-source-corridor-blocked", "Primary source surface corridor is blocked; alternate semantic escape surface is used.", {
          endpoint: "source",
          expected: intent.expectedSourceSide,
          actual: sourceSide
        })
      : null,
    target: options.target.has(targetSide)
      ? routeFinding("constrained-primary-target-corridor-blocked", "Primary target surface corridor is blocked; alternate semantic escape surface is used.", {
          endpoint: "target",
          expected: intent.expectedTargetSide,
          actual: targetSide
        })
      : null
  };
}

function singletonOffsetIsAlignmentTradeoff(endpoint, endpointsForRoute, counts) {
  const opposite = endpoint.endpoint === "source" ? endpointsForRoute.target : endpointsForRoute.source;
  if (!opposite?.side) return false;
  return (counts.get(endpointKey(opposite.nodeId, opposite.side)) ?? 0) > 1;
}

// T4 detector: a reciprocal pair should render as two parallel lines, so its two routes crossing
// each other is always a defect — a misaligned/jogged pair (e.g. model-inference route-cloud vs
// cloud-provider-result, which jog to different x at each end and swap sides). Pure function so the
// diagnostics overlay, the tests, and the sweep tool all share one implementation.
export function pairInternalCrossings(routes, relationships) {
  const results = [];
  for (const [a, b] of reciprocalPairsByAdjacency(relationships)) {
    const routeA = routes.get(a);
    const routeB = routes.get(b);
    if (!routeA || !routeB) continue;
    const crossings = crossingsBetween(routeA, routeB);
    if (crossings > 0) results.push({ a, b, crossings });
  }
  return results;
}

// T4 detector: when several routes leave the SAME node face and run in parallel gutter lanes, the
// line whose target is farthest should sit in the OUTERMOST lane so its long descent does not cross
// the shorter siblings' brackets (model-inference record-route targets the farthest node yet sits in
// the innermost lane, crossing route-local / local-provider-result). Flags a face whose
// farthest-target route is not its outermost lane.
// The perpendicular coordinate of the route's longest segment that runs PARALLEL to a face: the
// gutter lane. For a left/right face the parallel runs are vertical (constant x), so the lane is the
// x of the longest vertical segment; for a top/bottom face it is the y of the longest horizontal
// segment. Returns null when the route has no such run (a direct facing line, not a gutter lane).
function longestParallelRunCoordinate(points, vertical) {
  let bestLength = 0;
  let bestCoordinate = null;
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    const isParallel = vertical ? start.x === end.x : start.y === end.y;
    if (!isParallel) continue;
    const length = vertical ? Math.abs(end.y - start.y) : Math.abs(end.x - start.x);
    if (length > bestLength) {
      bestLength = length;
      bestCoordinate = vertical ? start.x : start.y;
    }
  }
  return bestCoordinate;
}

export function laneOrderViolations(plan, relationships) {
  const byId = relationshipById(relationships);
  const groups = new Map();
  for (const [id, route] of plan.routes) {
    const relationship = byId.get(id);
    if (!relationship) continue;
    const fromRect = plan.nodeRects.get(relationship.from);
    const toRect = plan.nodeRects.get(relationship.to);
    if (!fromRect || !toRect) continue;
    const side = sideForPoint(fromRect, route.points[0]);
    if (!side) continue;
    const vertical = side === "left" || side === "right";
    const faceCoord = side === "left" ? fromRect.x
      : side === "right" ? fromRect.x + fromRect.width
      : side === "top" ? fromRect.y
      : fromRect.y + fromRect.height;
    // The lane is the route's longest run PARALLEL to the face (the long vertical segment for a
    // left/right face); its perpendicular coordinate is the lane position. Using the route's
    // outermost coordinate instead would pick up the destination endpoint when source and target
    // sit in different columns. A route with no parallel run is a direct facing line, not a gutter
    // lane participant, so it is skipped.
    const laneCoord = longestParallelRunCoordinate(route.points, vertical);
    if (laneCoord === null) continue;
    const offset = Math.abs(laneCoord - faceCoord);
    const fromCentre = vertical ? fromRect.y + fromRect.height / 2 : fromRect.x + fromRect.width / 2;
    const toCentre = vertical ? toRect.y + toRect.height / 2 : toRect.x + toRect.width / 2;
    const targetDistance = Math.abs(toCentre - fromCentre);
    const key = endpointKey(relationship.from, side);
    if (!groups.has(key)) groups.set(key, { nodeId: relationship.from, side, items: [] });
    groups.get(key).items.push({ id, offset, targetDistance });
  }
  const violations = [];
  for (const { nodeId, side, items } of groups.values()) {
    if (items.length < 2) continue;
    const offsets = items.map((item) => item.offset);
    if (Math.max(...offsets) - Math.min(...offsets) < LANE_OFFSET_EPSILON) continue; // no distinct lanes
    const farthest = items.slice().sort((a, b) => b.targetDistance - a.targetDistance)[0];
    // The farthest target should be outermost. Report a violation only when the farthest route
    // actually CROSSES a sibling that sits outside it — i.e. the mis-ordering produces a visible
    // crossing. A lane that is ranked "wrong" but never overlaps a sibling is not a visible defect
    // (calibration showed the pure rank rule flags ~75% rank-only non-crossings), so it is ignored.
    const farthestRoute = plan.routes.get(farthest.id);
    if (!farthestRoute) continue;
    let crossedSibling = null;
    for (const sibling of items) {
      if (sibling.id === farthest.id) continue;
      if (sibling.offset - farthest.offset <= LANE_OFFSET_EPSILON) continue; // sibling not outside farthest
      const siblingRoute = plan.routes.get(sibling.id);
      if (siblingRoute && crossingsBetween(farthestRoute, siblingRoute) > 0) {
        crossedSibling = sibling;
        break;
      }
    }
    if (crossedSibling) {
      violations.push({ nodeId, side, farthest: farthest.id, outermost: crossedSibling.id });
    }
  }
  return violations;
}

export function diagnosePlannedRoutes(plan, relationships, options = {}) {
  const relationshipMap = relationshipById(relationships);
  const endpoints = endpointRecords(plan, relationships);
  const counts = endpointCounts(endpoints);
  const endpointByRelationship = new Map();
  for (const endpoint of endpoints) {
    const record = endpointByRelationship.get(endpoint.relationshipId) ?? {};
    record[endpoint.endpoint] = endpoint;
    endpointByRelationship.set(endpoint.relationshipId, record);
  }

  const routeDiagnostics = [];
  const findings = [];
  for (const [relationshipId, route] of plan.routes) {
    const relationship = relationshipMap.get(relationshipId);
    if (!relationship) continue;
    const sourceRect = plan.nodeRects.get(relationship.from);
    const targetRect = plan.nodeRects.get(relationship.to);
    if (!sourceRect || !targetRect) continue;
    const intent = deriveRouteIntent({
      relationship,
      fromRect: sourceRect,
      toRect: targetRect,
      fromLaneIndex: plan.laneIndexByNode.get(relationship.from),
      toLaneIndex: plan.laneIndexByNode.get(relationship.to),
      fromRowIndex: plan.rowIndexByNode.get(relationship.from),
      toRowIndex: plan.rowIndexByNode.get(relationship.to)
    });
    const endpointsForRoute = endpointByRelationship.get(relationshipId) ?? {};
    const source = endpointsForRoute.source;
    const target = endpointsForRoute.target;
    const semanticDeviation = semanticSurfaceDeviationConstraint(plan, relationship, sourceRect, targetRect, intent, source?.side, target?.side);
    const sourceCount = counts.get(endpointKey(relationship.from, source?.side)) ?? 0;
    const targetCount = counts.get(endpointKey(relationship.to, target?.side)) ?? 0;
    const sourceOffset = source ? sideOffset(source.rect, source.side, source.point) : 0;
    const targetOffset = target ? sideOffset(target.rect, target.side, target.point) : 0;
    const routeFindings = [];
    const routeConstraints = [];

    for (const endpoint of [source, target]) {
      if (!endpoint?.side) {
        routeFindings.push(routeFinding("endpoint-off-surface", "Endpoint does not land on a node surface.", { endpoint: endpoint?.endpoint }));
        continue;
      }
      const count = counts.get(endpointKey(endpoint.nodeId, endpoint.side)) ?? 0;
      const capacity = surfaceCapacity(endpoint.rect, endpoint.side);
      const offset = sideOffset(endpoint.rect, endpoint.side, endpoint.point);
      if (count === 1 && Math.abs(offset) > POINT_EPSILON) {
        if (singletonOffsetIsAlignmentTradeoff(endpoint, endpointsForRoute, counts)) {
          routeConstraints.push(routeFinding("constrained-singleton-aligned-to-busy-opposite-side", "Singleton endpoint is offset to align with a busier opposite side.", {
            endpoint: endpoint.endpoint,
            nodeId: endpoint.nodeId,
            side: endpoint.side,
            offset
          }));
        } else {
          routeFindings.push(routeFinding("singleton-endpoint-off-center", "Only one endpoint uses this node side but it is not centered.", {
            endpoint: endpoint.endpoint,
            nodeId: endpoint.nodeId,
            side: endpoint.side,
            offset
          }));
        }
      }
      if (count > capacity) {
        routeFindings.push(routeFinding("surface-over-capacity", "A node side has more endpoints than its surface capacity.", {
          endpoint: endpoint.endpoint,
          nodeId: endpoint.nodeId,
          side: endpoint.side,
          count,
          capacity
        }));
      }
    }

    if (source?.side && source.side !== intent.expectedSourceSide) {
      const constraint = semanticDeviation.source ?? constrainedSurfaceDeviation(plan, relationship, source, intent.expectedSourceSide, intent.expectedSourceSide, intent.expectedTargetSide, counts);
      if (constraint) {
        routeConstraints.push(constraint);
      } else {
        routeFindings.push(routeFinding("non-facing-source-surface", "Source route does not use the expected facing surface.", {
          expected: intent.expectedSourceSide,
          actual: source.side
        }));
      }
    }
    if (target?.side && target.side !== intent.expectedTargetSide) {
      const constraint = semanticDeviation.target ?? constrainedSurfaceDeviation(plan, relationship, target, intent.expectedTargetSide, intent.expectedSourceSide, intent.expectedTargetSide, counts);
      if (constraint) {
        routeConstraints.push(constraint);
      } else {
        routeFindings.push(routeFinding("non-facing-target-surface", "Target route does not use the expected facing surface.", {
          expected: intent.expectedTargetSide,
          actual: target.side
        }));
      }
    }
    if ((route.sharedSegments ?? 0) > 0) {
      routeFindings.push(routeFinding("shared-route-segment", "Route shares visible segment geometry with another route.", {
        sharedSegments: route.sharedSegments,
        sharedSegmentLength: route.sharedSegmentLength
      }));
    }
    if ((route.selfOverlappingSegments ?? 0) > 0) {
      routeFindings.push(routeFinding("self-overlapping-route", "Route doubles back over itself.", {
        selfOverlappingSegments: route.selfOverlappingSegments
      }));
    }
    if ((route.endpointNodeTraversals ?? 0) > 0) {
      routeFindings.push(routeFinding("endpoint-node-traversal", "Route traverses a source or target node body.", {
        endpointNodeTraversals: route.endpointNodeTraversals
      }));
    }
    for (const warning of route.warnings ?? []) {
      routeFindings.push(routeFinding(`route-warning:${warning.code}`, warning.message));
    }

    const diagnostic = {
      relationshipId,
      role: intent.role,
      step: relationship.displayIndex,
      kind: relationship.kind,
      returnOf: relationship.returnOf,
      outcome: relationship.outcome,
      laneDirection: intent.laneDirection,
      rowDirection: intent.rowDirection,
      from: relationship.from,
      to: relationship.to,
      sourceSide: source?.side ?? "",
      targetSide: target?.side ?? "",
      expectedSourceSide: intent.expectedSourceSide,
      expectedTargetSide: intent.expectedTargetSide,
      sourceSideUseCount: sourceCount,
      targetSideUseCount: targetCount,
      sourceSideCapacity: source ? surfaceCapacity(source.rect, source.side) : 0,
      targetSideCapacity: target ? surfaceCapacity(target.rect, target.side) : 0,
      sourceOffset,
      targetOffset,
      bends: route.bends ?? 0,
      crossings: route.crossings ?? 0,
      repeatedCrossings: route.repeatedCrossings ?? 0,
      sharedSegments: route.sharedSegments ?? 0,
      selfOverlappingSegments: route.selfOverlappingSegments ?? 0,
      hopCount: hopCount(route),
      constraints: routeConstraints,
      findings: routeFindings
    };
    routeDiagnostics.push(diagnostic);
    findings.push(...routeFindings.map((finding) => ({ relationshipId, ...finding })));
  }

  const closeParallelRuns = closeParallelRunCount(plan.routes);
  if (closeParallelRuns > (options.closeParallelRunBudget ?? 0)) {
    findings.push(routeFinding("close-parallel-route-runs", "Routes run close together long enough to read as the same channel.", {
      closeParallelRuns
    }));
  }

  const pairCrossings = pairInternalCrossings(plan.routes, relationships);
  for (const pair of pairCrossings) {
    findings.push(routeFinding("pair-internal-crossing", "A reciprocal pair's two lines cross each other instead of running parallel.", {
      relationshipId: pair.a,
      pairWith: pair.b,
      crossings: pair.crossings
    }));
  }
  const laneOrder = laneOrderViolations(plan, relationships);
  for (const violation of laneOrder) {
    findings.push(routeFinding("lane-order-violation", "A gutter lane's farthest-target line is not in the outermost lane, so it crosses shorter siblings.", {
      nodeId: violation.nodeId,
      side: violation.side,
      farthest: violation.farthest,
      outermost: violation.outermost
    }));
  }

  return {
    routes: routeDiagnostics,
    findings,
    metrics: {
      routes: routeDiagnostics.length,
      findings: findings.length,
      constraints: routeDiagnostics.reduce((sum, route) => sum + route.constraints.length, 0),
      closeParallelRuns,
      pairInternalCrossings: pairCrossings.reduce((sum, pair) => sum + pair.crossings, 0),
      laneOrderViolations: laneOrder.length,
      bends: routeDiagnostics.reduce((sum, route) => sum + route.bends, 0),
      sharedSegments: routeDiagnostics.reduce((sum, route) => sum + route.sharedSegments, 0),
      repeatedCrossings: routeDiagnostics.reduce((sum, route) => sum + route.repeatedCrossings, 0),
      hops: routeDiagnostics.reduce((sum, route) => sum + route.hopCount, 0)
    }
  };
}
