import { segmentIntersectsRect } from "./routeGeometry.js";
import { anchorFor, surfaceCapacity } from "./routePorts.js";
import { deriveRouteIntent, semanticSurfaceOptions } from "./routeIntent.js";

const POINT_EPSILON = 1;
const CLOSE_SEGMENT_DISTANCE = 10;
const CLOSE_SEGMENT_OVERLAP = 72;

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
          if (overlap < CLOSE_SEGMENT_OVERLAP) continue;
          if (Math.abs(left.line - right.line) <= CLOSE_SEGMENT_DISTANCE) count += 1;
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

  return {
    routes: routeDiagnostics,
    findings,
    metrics: {
      routes: routeDiagnostics.length,
      findings: findings.length,
      constraints: routeDiagnostics.reduce((sum, route) => sum + route.constraints.length, 0),
      closeParallelRuns,
      bends: routeDiagnostics.reduce((sum, route) => sum + route.bends, 0),
      sharedSegments: routeDiagnostics.reduce((sum, route) => sum + route.sharedSegments, 0),
      repeatedCrossings: routeDiagnostics.reduce((sum, route) => sum + route.repeatedCrossings, 0),
      hops: routeDiagnostics.reduce((sum, route) => sum + route.hopCount, 0)
    }
  };
}
