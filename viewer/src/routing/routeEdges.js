import { HOP_RADIUS, pathToSvgWithHops } from "./routeRendering.js";
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
import { simplifyOrthogonalPoints } from "./routeRendering.js";
import {
  anchorFor,
  offsetForEndpointOrder,
  PORT_STUB,
  portFor,
  sideVector,
  surfaceCapacity
} from "./routePorts.js";
import { getCachedRawRoutes, routeCacheKey, setCachedRawRoutes } from "./routeCache.js";
import { normalizeRouteStyle } from "./routeStyle.js";
import { estimatedLabelBox, withReadableLabel } from "./routeLabels.js";
import { edgeCorridors, freeSpaceCorridors } from "./routeCorridors.js";
import { createRouteCandidateFactory } from "./routeCandidateBuilders.js";
import { selectRouteCandidate } from "./routeStrategies.js";
import { relieveCrowdedSurfaces, optimizeMountAssignments } from "./routeMountModel.js";
import { CANVAS_INSET, ROUTE_COST_WEIGHTS, RECIPROCAL_PARALLEL_OFFSET, rectCenter } from "./routeConstants.js";
import { reciprocalPairsByAdjacency } from "./routeReciprocal.js";

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

function renderOrthogonalRoute(route, allRoutes) {
  const sharedSegmentStats = finalSharedSegmentStats(route, allRoutes);
  return withReadableLabel({
    ...route,
    d: pathToSvgWithHops(route.points, allRoutes),
    sampleBounds: boundsForPoints(route.samples),
    sharedSegments: sharedSegmentStats.count,
    sharedSegmentLength: sharedSegmentStats.length,
    style: "orthogonal"
  });
}

function renderedAxisAlignedSegments(points) {
  const segments = [];
  for (let index = 0; index < (points?.length ?? 0) - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    if (start.x === end.x) {
      segments.push({ orientation: "vertical", line: start.x, min: Math.min(start.y, end.y), max: Math.max(start.y, end.y) });
    } else if (start.y === end.y) {
      segments.push({ orientation: "horizontal", line: start.y, min: Math.min(start.x, end.x), max: Math.max(start.x, end.x) });
    }
  }
  return segments;
}

function finalSharedSegmentStats(route, allRoutes) {
  const routeSegments = renderedAxisAlignedSegments(route.points);
  let count = 0;
  let length = 0;
  for (const otherRoute of allRoutes) {
    if (otherRoute === route) continue;
    for (const left of routeSegments) {
      for (const right of renderedAxisAlignedSegments(otherRoute.points)) {
        if (left.orientation !== right.orientation || left.line !== right.line) continue;
        const overlap = Math.min(left.max, right.max) - Math.max(left.min, right.min);
        if (overlap > 1) {
          count += 1;
          length += overlap;
        }
      }
    }
  }
  return { count, length };
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

function createEndpointSideUsage() {
  const counts = new Map();
  return {
    isAvailable(nodeId, side, rect) {
      if (!nodeId || !side || !rect) return true;
      return (counts.get(sideEndpointKey(nodeId, side)) ?? 0) < surfaceCapacity(rect, side);
    },
    mark(nodeId, side) {
      if (!nodeId || !side) return;
      const key = sideEndpointKey(nodeId, side);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
  };
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
  if (nextPoints.length === 2) {
    const adjacent = nextPoints[adjacentIndex];
    const port = (rawOffset === 0 ? portFor(rect, side, 18, 0) : portFor(rect, side, 18, rawOffset)).port;
    const elbow = side === "left" || side === "right"
      ? { x: port.x, y: adjacent.y }
      : { x: adjacent.x, y: port.y };
    return endpointIndex === 0
      ? [anchor, port, elbow, adjacent]
      : [adjacent, elbow, port, anchor];
  }
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
  const nextPoints = route.style === "spline" ? points : simplifyOrthogonalPoints(orthogonalizedPoints(points));
  const samples = route.style === "spline" && controls?.length === 2
    ? [nextPoints[0], ...sampleCubic(nextPoints[0], controls[0], controls[1], nextPoints.at(-1), 32)]
    : route.style === "straight"
      ? sampleLine(nextPoints[0], nextPoints.at(-1), 18)
      : lineSamples(nextPoints);
  const label = samples[Math.floor(samples.length / 2)] ?? nextPoints[Math.floor(nextPoints.length / 2)] ?? { x: 0, y: 0 };
  return {
    ...route,
    d: route.style === "spline" && controls?.length === 2
      ? `M ${nextPoints[0].x} ${nextPoints[0].y} C ${controls[0].x} ${controls[0].y} ${controls[1].x} ${controls[1].y} ${nextPoints.at(-1).x} ${nextPoints.at(-1).y}`
      : nextPoints.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x} ${point.y}`).join(" "),
    points: nextPoints,
    controls,
    samples,
    sampleBounds: boundsForPoints([...nextPoints, ...samples]),
    bends: bendCount(nextPoints),
    labelX: label.x,
    labelY: label.y
  };
}

function orthogonalizedPoints(points) {
  if (!points?.length) return points;
  const nextPoints = [points[0]];
  for (let index = 1; index < points.length; index += 1) {
    const previous = nextPoints[nextPoints.length - 1];
    const point = points[index];
    if (previous.x !== point.x && previous.y !== point.y) {
      nextPoints.push({ x: point.x, y: previous.y });
    }
    nextPoints.push(point);
  }
  return nextPoints;
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

function collapseAlignedOpposingSurfaceRoute(route, firstSide, lastSide, relationship, input) {
  const first = route.points[0];
  const last = route.points.at(-1);
  if ((firstSide === "left" || firstSide === "right") && (lastSide === "left" || lastSide === "right") && first.y === last.y) {
    const collapsed = routeWithPoints(route, [first, last], route.controls);
    return routeCollidesWithNonEndpoints(collapsed, relationship, input) ? route : collapsed;
  }
  if ((firstSide === "top" || firstSide === "bottom") && (lastSide === "top" || lastSide === "bottom") && first.x === last.x) {
    const collapsed = routeWithPoints(route, [first, last], route.controls);
    return routeCollidesWithNonEndpoints(collapsed, relationship, input) ? route : collapsed;
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

function recenteredWithoutNewSharedSegments(route, nextRoute, otherRoutes, relationship, input) {
  const nextCollisions = relationship && input ? nonEndpointNodeCollisionCount(nextRoute, relationship, input) : 0;
  const currentCollisions = relationship && input ? nonEndpointNodeCollisionCount(route, relationship, input) : 0;
  return nextCollisions <= currentCollisions && sharedSegmentCount(nextRoute, otherRoutes) <= sharedSegmentCount(route, otherRoutes)
    ? nextRoute
    : route;
}

function routeWithFewestSharedSegments(routes, otherRoutes) {
  return routes.filter(Boolean).sort((left, right) => (
    sharedSegmentCount(left, otherRoutes) - sharedSegmentCount(right, otherRoutes) ||
    left.bends - right.bends
  ))[0];
}

function nonEndpointNodeCollisionCount(route, relationship, input) {
  if (!relationship) return 0;
  let count = 0;
  for (const nodeId of input.visibleNodeIds ?? []) {
    if (nodeId === relationship.from || nodeId === relationship.to) continue;
    const rect = input.nodeRects.get(nodeId);
    if (rect && routeIntersectsRect(route, rect, 0)) count += 1;
  }
  return count;
}

function routeWithBestCleanupCandidate(routes, otherRoutes, relationship, input) {
  return routes.filter(Boolean).sort((left, right) => (
    nonEndpointNodeCollisionCount(left, relationship, input) - nonEndpointNodeCollisionCount(right, relationship, input) ||
    sharedSegmentCount(left, otherRoutes) - sharedSegmentCount(right, otherRoutes) ||
    left.bends - right.bends
  ))[0];
}

function alignedFacingEndpointRoute(route, relationship, input, endpointGroups) {
  if (!route?.points?.length || route.points.length < 2 || !relationship) return route;
  const fromRect = input.nodeRects.get(relationship.from);
  const toRect = input.nodeRects.get(relationship.to);
  if (!fromRect || !toRect || fromRect.fixedPorts || toRect.fixedPorts) return route;
  const startSide = endpointSide(fromRect, route.points[0]);
  const endSide = endpointSide(toRect, route.points.at(-1));
  // Aligning both endpoints to a shared coordinate only produces a straight edge
  // when the two sides DIRECTLY FACE each other (right->left with the source left
  // of the target, or bottom->top with the source above it). Applying it to
  // same-direction sides (e.g. bottom->bottom) forces a coordinate match that
  // bends the edge instead of straightening it.
  const horizontalFacing =
    (startSide === "right" && endSide === "left" && fromRect.x < toRect.x) ||
    (startSide === "left" && endSide === "right" && fromRect.x > toRect.x);
  const verticalFacing =
    (startSide === "bottom" && endSide === "top" && fromRect.y < toRect.y) ||
    (startSide === "top" && endSide === "bottom" && fromRect.y > toRect.y);
  if (!horizontalFacing && !verticalFacing) return route;
  const sourceCount = endpointGroups.get(sideEndpointKey(relationship.from, startSide))?.length ?? 1;
  const targetCount = endpointGroups.get(sideEndpointKey(relationship.to, endSide))?.length ?? 1;
  const coordinateDelta = horizontalFacing
    ? Math.abs(route.points[0].y - route.points.at(-1).y)
    : Math.abs(route.points[0].x - route.points.at(-1).x);
  if (coordinateDelta < 1 || coordinateDelta > PORT_STUB) return route;

  // Move the sparser side's endpoint to match the busier (tightly packed) side's
  // fixed mount; moving the busy side instead would collide with its neighbours
  // and the straight candidate would be rejected, leaving the dogleg in place.
  const alignSource = sourceCount <= targetCount;
  const endpointIndex = alignSource ? 0 : route.points.length - 1;
  const rect = alignSource ? fromRect : toRect;
  const side = alignSource ? startSide : endSide;
  const anchor = alignSource ? route.points.at(-1) : route.points[0];
  const center = rectCenter(rect);
  const rawOffset = horizontalFacing ? anchor.y - center.y : anchor.x - center.x;
  const nextRoute = offsetEndpointRoute(route, endpointIndex, rect, side, rawOffset);
  return routeWithPoints(nextRoute, [nextRoute.points[0], nextRoute.points.at(-1)], nextRoute.controls);
}

function endpointStubRoute(route, relationship, input, endpointIndex) {
  if (!route.points?.length || route.points.length < 3) return route;
  const nodeId = endpointIndex === 0 ? relationship.from : relationship.to;
  const rect = input.nodeRects.get(nodeId);
  if (!rect) return route;
  const points = route.points.map((point) => ({ ...point }));
  const anchorIndex = endpointIndex === 0 ? 0 : points.length - 1;
  const adjacentIndex = endpointIndex === 0 ? 1 : points.length - 2;
  const elbowIndex = endpointIndex === 0 ? 2 : points.length - 3;
  const anchor = points[anchorIndex];
  const adjacent = points[adjacentIndex];
  const side = endpointSide(rect, anchor);
  if (!side || !adjacent) return route;
  const currentStubLength = Math.hypot(anchor.x - adjacent.x, anchor.y - adjacent.y);
  if (currentStubLength >= PORT_STUB) return route;
  const vector = sideVector(side);
  const nextAdjacent = {
    x: anchor.x + vector.x * PORT_STUB,
    y: anchor.y + vector.y * PORT_STUB
  };
  const oldAdjacent = points[adjacentIndex];
  points[adjacentIndex] = nextAdjacent;
  const elbow = points[elbowIndex];
  if (elbow) {
    if ((side === "left" || side === "right") && elbow.x === oldAdjacent.x) {
      elbow.x = nextAdjacent.x;
    }
    if ((side === "top" || side === "bottom") && elbow.y === oldAdjacent.y) {
      elbow.y = nextAdjacent.y;
    }
  }
  return routeWithPoints(route, points, route.controls);
}

function enforceEndpointStubs(plannedRawRoutes, input) {
  const relationshipById = new Map(input.relationships.map((relationship) => [relationship.id, relationship]));
  return plannedRawRoutes.map(([relationshipId, route]) => {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || route.style === "spline" || route.style === "straight") return [relationshipId, route];
    let nextRoute = endpointStubRoute(route, relationship, input, 0);
    nextRoute = endpointStubRoute(nextRoute, relationship, input, nextRoute.points.length - 1);
    return [relationshipId, nextRoute];
  });
}

function routeWithEndpointStubs(route, relationship, input) {
  let nextRoute = endpointStubRoute(route, relationship, input, 0);
  nextRoute = endpointStubRoute(nextRoute, relationship, input, nextRoute.points.length - 1);
  return nextRoute;
}

function axisAlignedRouteSegments(route) {
  const segments = [];
  for (let index = 0; index < route.points.length - 1; index += 1) {
    const start = route.points[index];
    const end = route.points[index + 1];
    if (start.x === end.x) {
      segments.push({
        route,
        index,
        orientation: "vertical",
        line: start.x,
        min: Math.min(start.y, end.y),
        max: Math.max(start.y, end.y)
      });
    } else if (start.y === end.y) {
      segments.push({
        route,
        index,
        orientation: "horizontal",
        line: start.y,
        min: Math.min(start.x, end.x),
        max: Math.max(start.x, end.x)
      });
    }
  }
  return segments;
}

function closeParallelSegmentPair(routeById) {
  const entries = [...routeById];
  for (let leftRouteIndex = 0; leftRouteIndex < entries.length; leftRouteIndex += 1) {
    for (let rightRouteIndex = leftRouteIndex + 1; rightRouteIndex < entries.length; rightRouteIndex += 1) {
      const [leftId, leftRoute] = entries[leftRouteIndex];
      const [rightId, rightRoute] = entries[rightRouteIndex];
      for (const left of axisAlignedRouteSegments(leftRoute)) {
        for (const right of axisAlignedRouteSegments(rightRoute)) {
          if (left.orientation !== right.orientation) continue;
          const overlap = Math.min(left.max, right.max) - Math.max(left.min, right.min);
          const distance = Math.abs(left.line - right.line);
          const exactSharedSegment = distance === 0 && overlap > 1;
          const closeParallelSegment = overlap >= 72 && distance > 0 && distance <= 10;
          if (exactSharedSegment || closeParallelSegment) {
            return { leftId, rightId, left, right };
          }
        }
      }
    }
  }
  return null;
}

function shiftedInternalSegmentRoute(route, segment, delta) {
  if (segment.index <= 0 || segment.index >= route.points.length - 2) return null;
  const points = route.points.map((point) => ({ ...point }));
  if (segment.orientation === "vertical") {
    points[segment.index].x += delta;
    points[segment.index + 1].x += delta;
  } else {
    points[segment.index].y += delta;
    points[segment.index + 1].y += delta;
  }
  return routeWithPoints(route, points, route.controls);
}

function shiftedEndpointSegmentRoute(route, relationship, input, segment, delta) {
  const shiftsSourceSide = segment.index <= 1;
  const shiftsTargetSide = segment.index >= route.points.length - 3;
  const endpointIndex = shiftsSourceSide ? 0 : shiftsTargetSide ? route.points.length - 1 : -1;
  if (endpointIndex < 0) return null;
  const nodeId = shiftsSourceSide ? relationship.from : relationship.to;
  const rect = input.nodeRects.get(nodeId);
  if (!rect || rect.fixedPorts) return null;
  const point = route.points[endpointIndex];
  const side = endpointSide(rect, point);
  if (!side) return null;
  const center = rectCenter(rect);
  const nextLine = segment.line + delta;
  const rawOffset = segment.orientation === "horizontal" && (side === "left" || side === "right")
    ? nextLine - center.y
    : segment.orientation === "vertical" && (side === "top" || side === "bottom")
      ? nextLine - center.x
      : null;
  if (rawOffset === null) return null;
  return offsetEndpointRoute(route, endpointIndex, rect, side, rawOffset);
}

function shiftedDirectEndpointRoute(route, relationship, input, segment, delta) {
  if (route.points.length !== 2 || segment.index !== 0) return null;
  const fromRect = input.nodeRects.get(relationship.from);
  const toRect = input.nodeRects.get(relationship.to);
  if (!fromRect || !toRect || fromRect.fixedPorts || toRect.fixedPorts) return null;
  const startSide = endpointSide(fromRect, route.points[0]);
  const endSide = endpointSide(toRect, route.points[1]);
  const nextLine = segment.line + delta;
  if (segment.orientation === "vertical" && (startSide === "top" || startSide === "bottom") && (endSide === "top" || endSide === "bottom")) {
    const sourceOffset = nextLine - rectCenter(fromRect).x;
    const targetOffset = nextLine - rectCenter(toRect).x;
    return offsetEndpointRoute(
      offsetEndpointRoute(route, 0, fromRect, startSide, sourceOffset),
      1,
      toRect,
      endSide,
      targetOffset
    );
  }
  if (segment.orientation === "horizontal" && (startSide === "left" || startSide === "right") && (endSide === "left" || endSide === "right")) {
    const sourceOffset = nextLine - rectCenter(fromRect).y;
    const targetOffset = nextLine - rectCenter(toRect).y;
    return offsetEndpointRoute(
      offsetEndpointRoute(route, 0, fromRect, startSide, sourceOffset),
      1,
      toRect,
      endSide,
      targetOffset
    );
  }
  return null;
}

function routePairIndex(relationship, relationships) {
  const pairKey = [relationship.from, relationship.to].sort().join("<->");
  let pairIndex = 0;
  for (const candidate of relationships) {
    if (candidate.id === relationship.id) return pairIndex;
    if ([candidate.from, candidate.to].sort().join("<->") === pairKey) pairIndex += 1;
  }
  return pairIndex;
}

function reroutedAgainstRouteSet(routeId, relationship, routeById, input) {
  const planner = routePlannerContext(input);
  const routeIndex = createRouteIndex();
  const usedRoutes = [];
  const rawRoutes = [];
  for (const [otherRouteId, otherRoute] of routeById) {
    if (otherRouteId === routeId) continue;
    usedRoutes.push(otherRoute.samples);
    rawRoutes.push(otherRoute);
    routeIndex.add(otherRoute, rawRoutes.length - 1);
  }
  const relationshipIndex = Math.max(0, input.relationships.findIndex((candidate) => candidate.id === relationship.id));
  return planner.edgePath(
    relationship,
    relationshipIndex,
    routePairIndex(relationship, input.relationships),
    usedRoutes,
    rawRoutes,
    routeIndex,
    { from: 0, to: 0 },
    null,
    input.style
  );
}

function routeCollidesWithNonEndpoints(route, relationship, input) {
  for (const nodeId of input.visibleNodeIds) {
    if (nodeId === relationship.from || nodeId === relationship.to) continue;
    const rect = input.nodeRects.get(nodeId);
    if (rect && routeIntersectsRect(route, rect, 0)) return true;
  }
  return false;
}

function routeHasEndpointTraversal(route, relationship, input) {
  for (const nodeId of [relationship.from, relationship.to]) {
    const rect = input.nodeRects.get(nodeId);
    if (!rect) continue;
    if (route.samples.some((point) => point.x > rect.x && point.x < rect.x + rect.width && point.y > rect.y && point.y < rect.y + rect.height)) {
      return true;
    }
  }
  return false;
}

function routeEndpointsArePerpendicular(route, relationship, input) {
  const endpoints = [
    { nodeId: relationship.from, pointIndex: 0, adjacentIndex: 1 },
    { nodeId: relationship.to, pointIndex: route.points.length - 1, adjacentIndex: route.points.length - 2 }
  ];
  for (const endpoint of endpoints) {
    const rect = input.nodeRects.get(endpoint.nodeId);
    const point = route.points[endpoint.pointIndex];
    const adjacent = route.points[endpoint.adjacentIndex];
    if (!rect || !point || !adjacent) continue;
    const side = endpointSide(rect, point);
    if (!side) return false;
    if ((side === "left" || side === "right") && point.y !== adjacent.y) return false;
    if ((side === "top" || side === "bottom") && point.x !== adjacent.x) return false;
  }
  return true;
}

function closeParallelRunCountForRoutes(routeById) {
  let count = 0;
  const entries = [...routeById];
  for (let leftRouteIndex = 0; leftRouteIndex < entries.length; leftRouteIndex += 1) {
    for (let rightRouteIndex = leftRouteIndex + 1; rightRouteIndex < entries.length; rightRouteIndex += 1) {
      for (const left of axisAlignedRouteSegments(entries[leftRouteIndex][1])) {
        for (const right of axisAlignedRouteSegments(entries[rightRouteIndex][1])) {
          if (left.orientation !== right.orientation) continue;
          const overlap = Math.min(left.max, right.max) - Math.max(left.min, right.min);
          if (overlap >= 72 && Math.abs(left.line - right.line) <= 10) count += 1;
        }
      }
    }
  }
  return count;
}

function closeParallelRunCountBetween(leftRoute, rightRoute) {
  let count = 0;
  for (const left of axisAlignedRouteSegments(leftRoute)) {
    for (const right of axisAlignedRouteSegments(rightRoute)) {
      if (left.orientation !== right.orientation) continue;
      const overlap = Math.min(left.max, right.max) - Math.max(left.min, right.min);
      if (overlap >= 72 && Math.abs(left.line - right.line) <= 10) count += 1;
    }
  }
  return count;
}

const ROUTE_SEPARATION_DISTANCES = [5, 7, 9, 11, 13, 15, 18, 24, 30, 36, 48, 60, -5, -7, -9, -11, -13, -15, -18, -24, -30, -36, -48, -60];

function crossingPairKey(leftRouteIndex, rightRouteIndex) {
  return leftRouteIndex < rightRouteIndex
    ? `${leftRouteIndex}:${rightRouteIndex}`
    : `${rightRouteIndex}:${leftRouteIndex}`;
}

function routeSetStats(routeById) {
  const routeEntries = [...routeById];
  const crossings = new Map();
  let sharedSegments = 0;
  for (let leftRouteIndex = 0; leftRouteIndex < routeEntries.length; leftRouteIndex += 1) {
    for (let rightRouteIndex = leftRouteIndex + 1; rightRouteIndex < routeEntries.length; rightRouteIndex += 1) {
      for (const left of axisAlignedRouteSegments(routeEntries[leftRouteIndex][1])) {
        for (const right of axisAlignedRouteSegments(routeEntries[rightRouteIndex][1])) {
          if (left.orientation === right.orientation) {
            if (left.line !== right.line) continue;
            const overlap = Math.min(left.max, right.max) - Math.max(left.min, right.min);
            if (overlap > 1) sharedSegments += 1;
            continue;
          }
          const horizontal = left.orientation === "horizontal" ? left : right;
          const vertical = left.orientation === "vertical" ? left : right;
          if (
            vertical.line > horizontal.min + HOP_RADIUS &&
            vertical.line < horizontal.max - HOP_RADIUS &&
            horizontal.line > vertical.min + HOP_RADIUS &&
            horizontal.line < vertical.max - HOP_RADIUS
          ) {
            const key = crossingPairKey(leftRouteIndex, rightRouteIndex);
            crossings.set(key, (crossings.get(key) ?? 0) + 1);
          }
        }
      }
    }
  }
  return {
    repeatedCrossings: [...crossings.values()].reduce((sum, count) => sum + Math.max(0, count - 1), 0),
    sharedSegments
  };
}

function routeSeparationScore({ nextCloseCount, nextStats, nextPairCloseCount, candidate, distance }) {
  return [
    nextCloseCount,
    nextStats.sharedSegments,
    nextStats.repeatedCrossings,
    nextPairCloseCount,
    candidate.bends,
    Math.abs(distance)
  ];
}

function isBetterRouteSeparation(left, right) {
  if (!right) return true;
  for (let index = 0; index < left.score.length; index += 1) {
    if (left.score[index] !== right.score[index]) return left.score[index] < right.score[index];
  }
  return false;
}

function totalBendsForRoutes(routeById) {
  return [...routeById.values()].reduce((sum, route) => sum + (route.bends ?? 0), 0);
}

function routeSetScore(routeById) {
  const stats = routeSetStats(routeById);
  return [
    closeParallelRunCountForRoutes(routeById),
    stats.sharedSegments,
    stats.repeatedCrossings,
    totalBendsForRoutes(routeById)
  ];
}

function isBetterRouteSet(left, right) {
  if (!right) return true;
  for (let index = 0; index < left.score.length; index += 1) {
    if (left.score[index] !== right.score[index]) return left.score[index] < right.score[index];
  }
  return false;
}

function cloneRouteById(routeById) {
  return new Map(routeById);
}

function separateCloseParallelRoutes(plannedRawRoutes, input) {
  const relationshipById = new Map(input.relationships.map((relationship) => [relationship.id, relationship]));
  const routeById = new Map(plannedRawRoutes);
  let bestRouteSet = { routes: cloneRouteById(routeById), score: routeSetScore(routeById) };
  const maxAttempts = Math.max(8, plannedRawRoutes.length * 12);
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const pair = closeParallelSegmentPair(routeById);
    if (!pair) break;
    const currentCloseCount = closeParallelRunCountForRoutes(routeById);
    const currentStats = routeSetStats(routeById);
    const currentPairCloseCount = closeParallelRunCountBetween(
      routeById.get(pair.leftId),
      routeById.get(pair.rightId)
    );
    const options = [
      { routeId: pair.leftId, segment: pair.left, otherLine: pair.right.line },
      { routeId: pair.rightId, segment: pair.right, otherLine: pair.left.line }
    ];
    let best = null;
    for (const option of options) {
      const route = routeById.get(option.routeId);
      const relationship = relationshipById.get(option.routeId);
      if (!route || !relationship) continue;
      const direction = option.segment.line >= option.otherLine ? 1 : -1;
      for (const distance of ROUTE_SEPARATION_DISTANCES) {
        const candidates = [
          shiftedDirectEndpointRoute(route, relationship, input, option.segment, distance * direction),
          shiftedInternalSegmentRoute(route, option.segment, distance * direction),
          shiftedEndpointSegmentRoute(route, relationship, input, option.segment, distance * direction)
        ].filter(Boolean).map((candidate) => routeWithEndpointStubs(candidate, relationship, input));
        if (Math.abs(distance) === ROUTE_SEPARATION_DISTANCES[0]) {
          const rerouted = reroutedAgainstRouteSet(option.routeId, relationship, routeById, input);
          if (rerouted) candidates.push(routeWithEndpointStubs(rerouted, relationship, input));
        }
        for (const candidate of candidates) {
          if (!routeEndpointsArePerpendicular(candidate, relationship, input)) continue;
          if (routeCollidesWithNonEndpoints(candidate, relationship, input)) continue;
          if (routeHasEndpointTraversal(candidate, relationship, input)) continue;
          routeById.set(option.routeId, candidate);
          const nextCloseCount = closeParallelRunCountForRoutes(routeById);
          const nextStats = routeSetStats(routeById);
          const nextPairCloseCount = closeParallelRunCountBetween(
            routeById.get(pair.leftId),
            routeById.get(pair.rightId)
          );
          routeById.set(option.routeId, route);
          if (nextStats.sharedSegments > currentStats.sharedSegments) continue;
          const improvesSharedSegments = nextStats.sharedSegments < currentStats.sharedSegments;
          const improvesCloseRuns = nextCloseCount < currentCloseCount;
          const improvesSelectedPair = nextPairCloseCount < currentPairCloseCount;
          if (!improvesSharedSegments && !improvesCloseRuns && !improvesSelectedPair) continue;
          if (nextStats.repeatedCrossings > currentStats.repeatedCrossings + 4) continue;
          const nextBest = {
            ...option,
            candidate,
            score: routeSeparationScore({ nextCloseCount, nextStats, nextPairCloseCount, candidate, distance })
          };
          if (isBetterRouteSeparation(nextBest, best)) {
            best = nextBest;
          }
        }
      }
    }
    if (!best) {
      break;
    }
    routeById.set(best.routeId, best.candidate);
    const nextRouteSet = { routes: cloneRouteById(routeById), score: routeSetScore(routeById) };
    if (isBetterRouteSet(nextRouteSet, bestRouteSet)) {
      bestRouteSet = nextRouteSet;
    }
  }
  return plannedRawRoutes.map(([relationshipId]) => [relationshipId, bestRouteSet.routes.get(relationshipId)]);
}

function alternateMiddleDoglegRoutes(route) {
  if (!route.points || route.points.length !== 5) return [];
  const [start, sourceStub, middleA, targetStub, end] = route.points;
  const horizontalEndpointDogleg = (
    start.y === sourceStub.y &&
    sourceStub.y === middleA.y &&
    middleA.x === targetStub.x &&
    targetStub.y === end.y
  );
  if (horizontalEndpointDogleg && sourceStub.x !== targetStub.x) {
    const gutterY = (sourceStub.y + targetStub.y) / 2;
    return [
      routeWithPoints(route, [
        start,
        sourceStub,
        { x: sourceStub.x, y: targetStub.y },
        targetStub,
        end
      ], route.controls),
      routeWithPoints(route, [
        start,
        sourceStub,
        { x: sourceStub.x, y: gutterY },
        { x: targetStub.x, y: gutterY },
        targetStub,
        end
      ], route.controls)
    ];
  }
  const verticalEndpointDogleg = (
    start.x === sourceStub.x &&
    sourceStub.x === middleA.x &&
    middleA.y === targetStub.y &&
    targetStub.x === end.x
  );
  if (verticalEndpointDogleg && sourceStub.y !== targetStub.y) {
    const gutterX = (sourceStub.x + targetStub.x) / 2;
    return [
      routeWithPoints(route, [
        start,
        sourceStub,
        { x: targetStub.x, y: sourceStub.y },
        targetStub,
        end
      ], route.controls),
      routeWithPoints(route, [
        start,
        sourceStub,
        { x: gutterX, y: sourceStub.y },
        { x: gutterX, y: targetStub.y },
        targetStub,
        end
      ], route.controls)
    ];
  }
  return [];
}

function endpointSpreadOffset(index, count, rect, side) {
  const sideLength = side === "left" || side === "right" ? rect.height : rect.width;
  return ((index + 1) / (count + 1) - 0.5) * sideLength;
}

function oppositeEndpointProjection(endpoint, routeById, input) {
  const oppositeNodeId = endpoint.endpointIndex === 0 ? endpoint.relationship.to : endpoint.relationship.from;
  const oppositeRect = input.nodeRects.get(oppositeNodeId);
  if (oppositeRect) {
    const center = rectCenter(oppositeRect);
    return endpoint.side === "top" || endpoint.side === "bottom" ? center.x : center.y;
  }
  return 0;
}

function oppositeRouteEndpointProjection(endpoint, routeById) {
  const route = routeById.get(endpoint.relationshipId);
  const oppositePoint = endpoint.endpointIndex === 0 ? route?.points.at(-1) : route?.points[0];
  if (!oppositePoint) return 0;
  return endpoint.side === "top" || endpoint.side === "bottom" ? oppositePoint.x : oppositePoint.y;
}

function orderedSurfaceEndpoints(endpoints, routeById, input) {
  return [...endpoints].sort((left, right) => (
    oppositeEndpointProjection(left, routeById, input) - oppositeEndpointProjection(right, routeById, input) ||
    (left.relationship.displayIndex ?? 0) - (right.relationship.displayIndex ?? 0) ||
    oppositeRouteEndpointProjection(left, routeById) - oppositeRouteEndpointProjection(right, routeById) ||
    left.relationshipId.localeCompare(right.relationshipId)
  ));
}

// Snap every facing reciprocal pair so its two endpoints share one coordinate and the run is
// straight. alignedFacingEndpointRoute needs to know how many edges share each node side (to move
// the sparser end onto the busier end's fixed mount), so an endpoint-group count map is built
// first. Extracted so spreadSharedSideEndpoints and the post-relief replay run the IDENTICAL
// alignment — a missing copy in the replay is exactly what left facing pairs kinked after relief.
function realignFacingEndpoints(routeById, relationshipById, input) {
  const endpointGroups = new Map();
  for (const [relationshipId, route] of routeById) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || relationship.relationshipType !== "flow" || !route?.points?.length) continue;
    for (const [nodeId, point] of [[relationship.from, route.points[0]], [relationship.to, route.points.at(-1)]]) {
      const rect = input.nodeRects.get(nodeId);
      const side = rect ? endpointSide(rect, point) : "";
      if (!rect || rect.fixedPorts || !sideNeedsPostSelectionCentering(side)) continue;
      const key = sideEndpointKey(nodeId, side);
      endpointGroups.set(key, [...(endpointGroups.get(key) ?? []), relationshipId]);
    }
  }
  for (const [relationshipId, route] of routeById) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship) continue;
    const otherRoutes = [...routeById]
      .filter(([otherRelationshipId]) => otherRelationshipId !== relationshipId)
      .map(([, otherRoute]) => otherRoute);
    const alignedRoute = alignedFacingEndpointRoute(route, relationship, input, endpointGroups);
    routeById.set(relationshipId, routeWithBestCleanupCandidate([
      route,
      alignedRoute,
      ...alternateMiddleDoglegRoutes(route),
      ...alternateMiddleDoglegRoutes(alignedRoute)
    ], otherRoutes, relationship, input));
  }
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
    orderedSurfaceEndpoints(endpoints, routeById, input).forEach((endpoint, index) => {
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
      const oppositeEndpointCount = endpointGroups.get(sideEndpointKey(oppositeNodeId, oppositeSide))?.length ?? 0;
      const canAlignOpposite = (
        oppositeRect &&
        !oppositeRect.fixedPorts &&
        sideNeedsPostSelectionCentering(oppositeSide) &&
        oppositeEndpointCount <= 1
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
      const otherRoutes = [...routeById]
        .filter(([otherRelationshipId]) => otherRelationshipId !== endpoint.relationshipId)
        .map(([, otherRoute]) => otherRoute);
      const candidates = [offsetRoute, alignedRoute].flatMap((candidateRoute) => {
        const firstSide = firstRect ? endpointSide(firstRect, candidateRoute.points[0]) : "";
        const lastSide = lastRect ? endpointSide(lastRect, candidateRoute.points.at(-1)) : "";
        const collapsedRoute = collapseAlignedOpposingSurfaceRoute(candidateRoute, firstSide, lastSide, endpoint.relationship, input);
        return [
          collapsedRoute,
          ...alternateMiddleDoglegRoutes(collapsedRoute)
        ];
      });
      routeById.set(endpoint.relationshipId, routeWithBestCleanupCandidate(candidates, otherRoutes, endpoint.relationship, input));
    });
  }
  realignFacingEndpoints(routeById, relationshipById, input);
  reorderSharedSurfaceMounts(routeById, relationshipById, input);
  routeReciprocalPairsParallel(routeById, relationshipById, input);
  reduceCrossingsBySurfaceSwaps(routeById, relationshipById, input);
  centerSoloReciprocalPairSurfaces(routeById, relationshipById, input);
  return plannedRawRoutes.map(([relationshipId]) => [relationshipId, routeById.get(relationshipId)]);
}

// A reciprocal pair (A->B and B->A) whose two endpoints occupy surfaces carrying
// nothing but that pair can get bunched at one end after parallel-pairing. Re-space
// them symmetrically across the full surface. BOTH shared surfaces are re-spaced
// together (atomically) so a straight pair stays straight — centering one end
// alone would bend it and trip the guard. Kept only if it adds no bends/collisions.
function centerSoloReciprocalPairSurfaces(routeById, relationshipById, input) {
  const surfaceCounts = new Map();
  for (const [relationshipId, route] of routeById) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || relationship.relationshipType !== "flow" || !route.points?.length) continue;
    for (const [nodeId, endpointIndex] of [[relationship.from, 0], [relationship.to, route.points.length - 1]]) {
      const rect = input.nodeRects.get(nodeId);
      const point = endpointIndex === 0 ? route.points[0] : route.points.at(-1);
      const side = rect ? endpointSide(rect, point) : "";
      if (!rect || rect.fixedPorts || !sideNeedsPostSelectionCentering(side)) continue;
      const key = sideEndpointKey(nodeId, side);
      surfaceCounts.set(key, (surfaceCounts.get(key) ?? 0) + 1);
    }
  }
  const byNodePair = new Map();
  for (const relationship of relationshipById.values()) {
    if (relationship.relationshipType !== "flow" || !routeById.has(relationship.id)) continue;
    const key = [relationship.from, relationship.to].sort().join("\0");
    if (!byNodePair.has(key)) byNodePair.set(key, []);
    byNodePair.get(key).push(relationship);
  }
  for (const group of byNodePair.values()) {
    if (group.length !== 2) continue;
    const [a, b] = group;
    if (a.from !== b.to || a.to !== b.from) continue;
    // Gather both routes' endpoints that sit on sole-pair surfaces.
    const targets = [];
    for (const relationshipId of [a.id, b.id]) {
      const route = routeById.get(relationshipId);
      const relationship = relationshipById.get(relationshipId);
      for (const [nodeId, endpointIndex] of [[relationship.from, 0], [relationship.to, route.points.length - 1]]) {
        const rect = input.nodeRects.get(nodeId);
        const point = endpointIndex === 0 ? route.points[0] : route.points.at(-1);
        const side = rect ? endpointSide(rect, point) : "";
        if (!rect || rect.fixedPorts || !sideNeedsPostSelectionCentering(side)) continue;
        const key = sideEndpointKey(nodeId, side);
        if (surfaceCounts.get(key) !== 2) continue;
        targets.push({ relationshipId, endpointIndex, rect, side, key });
      }
    }
    if (!targets.length) continue;
    const saved = [...new Set(targets.map((target) => target.relationshipId))].map((id) => [id, routeById.get(id)]);
    const beforeBends = saved.reduce((sum, [, route]) => sum + (route.bends ?? 0), 0);
    const bySurface = new Map();
    for (const target of targets) {
      if (!bySurface.has(target.key)) bySurface.set(target.key, []);
      bySurface.get(target.key).push(target);
    }
    for (const surfaceTargets of bySurface.values()) {
      if (surfaceTargets.length < 2) continue;
      const axis = surfaceTargets[0].side === "left" || surfaceTargets[0].side === "right" ? "y" : "x";
      const ordered = surfaceTargets
        .map((target) => {
          const route = routeById.get(target.relationshipId);
          const point = target.endpointIndex === 0 ? route.points[0] : route.points.at(-1);
          return { ...target, mount: point[axis] };
        })
        .sort((left, right) => left.mount - right.mount);
      ordered.forEach((target, index) => {
        const route = routeById.get(target.relationshipId);
        const offset = endpointSpreadOffset(index, ordered.length, target.rect, target.side);
        routeById.set(target.relationshipId, offsetEndpointRoute(route, target.endpointIndex, target.rect, target.side, offset));
      });
    }
    const afterBends = saved.reduce((sum, [id]) => sum + (routeById.get(id).bends ?? 0), 0);
    const collides = saved.some(([id]) => routeCollidesWithNonEndpoints(routeById.get(id), relationshipById.get(id), input));
    if (collides || afterBends > beforeBends) {
      for (const [id, route] of saved) routeById.set(id, route);
    }
  }
}

// Count visible segment overlaps that involve one of `ids` (each unordered pair counted
// once), so a distribution move that slides a mount onto another route's lane is detectable.
function sharedSegmentCountInvolving(routeById, ids) {
  const idSet = new Set(ids);
  let total = 0;
  for (const id of ids) {
    const segments = axisAlignedSegments(routeById.get(id));
    for (const [otherId, otherRoute] of routeById) {
      if (otherId === id) continue;
      if (idSet.has(otherId) && otherId < id) continue;
      const otherSegments = axisAlignedSegments(otherRoute);
      for (const segment of segments) {
        for (const otherSegment of otherSegments) {
          if (sharedSegmentLength(segment, otherSegment) > 1) total += 1;
        }
      }
    }
  }
  return total;
}

// Shared guard for the distribution passes: run `applyMoves` (which mutates the routes named
// in `ids` within routeById), then keep the result only if it added no bend, no node
// collision, and no shared visible segment; otherwise restore the saved routes. This is the
// one place the "a redistribution may only refine" policy lives.
function keepMountMovesUnlessWorse(routeById, relationshipById, input, ids, applyMoves) {
  const saved = ids.map((id) => [id, routeById.get(id)]);
  const beforeBends = saved.reduce((sum, [, route]) => sum + (route.bends ?? 0), 0);
  const beforeShared = sharedSegmentCountInvolving(routeById, ids);
  applyMoves();
  const afterBends = ids.reduce((sum, id) => sum + (routeById.get(id).bends ?? 0), 0);
  const collides = ids.some((id) => routeCollidesWithNonEndpoints(routeById.get(id), relationshipById.get(id), input));
  if (collides || afterBends > beforeBends || sharedSegmentCountInvolving(routeById, ids) > beforeShared) {
    for (const [id, route] of saved) routeById.set(id, route);
    return false;
  }
  return true;
}

// Post-pass: even out STRAIGHT FACING runs between two adjacent nodes. When a node pair
// exchanges several straight reciprocal runs across one facing surface-pair (e.g. the
// Unified Pipeline <-> Memory request/return/ingest lines, all horizontal between
// UP.right and Memory.left), distributeSurfaceMountUnits cannot spread them: moving one
// node's mount alone tilts the straight run, so its guard reverts. Here both ends move
// together — each run is re-homed to an even slot by setting the SAME perpendicular
// coordinate on both endpoints, so the line stays straight while the set spreads. Runs in
// the group are ordered like the rest of the router (opposite-node centre, then display
// index) which keeps reciprocal partners adjacent. Guarded: reverts if it adds a bend, a
// node collision, or a shared segment.
function distributeFacingReciprocalSurfaces(routeById, relationshipById, input) {
  // How many flow endpoints terminate on each node face. A facing group is only re-spaced
  // when both its faces carry NOTHING but that group's runs — otherwise spreading the facing
  // runs in isolation ignores the other mounts and unbalances a mixed hub face.
  const faceOccupancy = new Map();
  for (const [relationshipId, route] of routeById) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || relationship.relationshipType !== "flow" || !route.points?.length) continue;
    for (const [nodeId, point] of [[relationship.from, route.points[0]], [relationship.to, route.points.at(-1)]]) {
      const rect = input.nodeRects.get(nodeId);
      const side = rect ? endpointSide(rect, point) : "";
      if (!rect || !side) continue;
      const faceKey = sideEndpointKey(nodeId, side);
      faceOccupancy.set(faceKey, (faceOccupancy.get(faceKey) ?? 0) + 1);
    }
  }
  const groups = new Map();
  for (const [relationshipId, route] of routeById) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || relationship.relationshipType !== "flow") continue;
    if (!route.points || route.points.length !== 2) continue; // only clean straight runs
    const fromRect = input.nodeRects.get(relationship.from);
    const toRect = input.nodeRects.get(relationship.to);
    if (!fromRect || !toRect || fromRect.fixedPorts || toRect.fixedPorts) continue;
    const [start, end] = route.points;
    const fromSide = endpointSide(fromRect, start);
    const toSide = endpointSide(toRect, end);
    if (!sideNeedsPostSelectionCentering(fromSide) || !sideNeedsPostSelectionCentering(toSide)) continue;
    const horizontal = start.y === end.y && (fromSide === "left" || fromSide === "right") && (toSide === "left" || toSide === "right");
    const vertical = start.x === end.x && (fromSide === "top" || fromSide === "bottom") && (toSide === "top" || toSide === "bottom");
    if (!horizontal && !vertical) continue;
    const axis = horizontal ? "y" : "x";
    // Key by the shared surface-pair (direction-independent) so both round-trip directions land together.
    const key = [sideEndpointKey(relationship.from, fromSide), sideEndpointKey(relationship.to, toSide)].sort().join("|");
    if (!groups.has(key)) {
      // Distribution span = the overlap of the two faces along the run axis (where a straight run is valid on both).
      const fromStart = axis === "y" ? fromRect.y : fromRect.x;
      const fromLen = axis === "y" ? fromRect.height : fromRect.width;
      const toStart = axis === "y" ? toRect.y : toRect.x;
      const toLen = axis === "y" ? toRect.height : toRect.width;
      const lo = Math.max(fromStart, toStart);
      const hi = Math.min(fromStart + fromLen, toStart + toLen);
      groups.set(key, { axis, lo, hi, runs: [] });
    }
    const group = groups.get(key);
    if (group.axis !== axis) continue;
    const oppositeRect = toRect;
    const oppositeCenter = axis === "y" ? oppositeRect.y + oppositeRect.height / 2 : oppositeRect.x + oppositeRect.width / 2;
    group.runs.push({ relationshipId, axis, current: start[axis], oppositeCenter, displayIndex: relationship.displayIndex ?? 0 });
  }

  for (const [key, { axis, lo, hi, runs }] of groups.entries()) {
    if (runs.length < 2 || hi <= lo) continue;
    // Only re-space when both facing surfaces carry nothing but this group's runs; a mixed
    // face (facing runs sharing a surface with unrelated mounts) is left to the per-face pass.
    if (key.split("|").some((faceKey) => (faceOccupancy.get(faceKey) ?? 0) !== runs.length)) continue;
    runs.sort((left, right) =>
      left.oppositeCenter - right.oppositeCenter ||
      left.displayIndex - right.displayIndex ||
      left.relationshipId.localeCompare(right.relationshipId));
    const targets = runs.map((run, index) => lo + ((index + 1) / (runs.length + 1)) * (hi - lo));
    if (runs.every((run, index) => Math.abs(run.current - targets[index]) < 0.5)) continue;

    keepMountMovesUnlessWorse(routeById, relationshipById, input, runs.map((run) => run.relationshipId), () => {
      runs.forEach((run, index) => {
        const route = routeById.get(run.relationshipId);
        const points = route.points.map((point) => ({ ...point, [axis]: targets[index] }));
        routeById.set(run.relationshipId, routeWithPoints(route, points, route.controls));
      });
    });
  }
}

// Post-pass: even out mount DISTRIBUTION on every shared surface. The earlier passes
// spread endpoints, but routeReciprocalPairsParallel then re-pins each return edge a
// fixed gap from its request edge, ignoring the return's own slot — so a face carrying
// several reciprocal pairs ends up with each pair bunched and the pair *centres* crowded
// at one end (the maintainer's "uneven mount distribution" / T1). Here a reciprocal pair
// counts as ONE unit (kept parallel by translating both mounts rigidly) and a lone edge
// as one unit; the unit CENTRES are spread evenly with the same endpointSpreadOffset
// fractions the rest of the router uses. A face with a single unit lands at offset 0, so
// this also centres lone mounts (T2). Applied per face and reverted if it adds a bend or
// a node collision, matching the surrounding passes' guard.
function distributeSurfaceMountUnits(routeById, relationshipById, input) {
  const groups = new Map();
  for (const [relationshipId, route] of routeById) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || relationship.relationshipType !== "flow" || !route.points?.length) continue;
    for (const [nodeId, endpointIndex] of [[relationship.from, 0], [relationship.to, route.points.length - 1]]) {
      const rect = input.nodeRects.get(nodeId);
      const point = endpointIndex === 0 ? route.points[0] : route.points.at(-1);
      const side = rect ? endpointSide(rect, point) : "";
      if (!rect || rect.fixedPorts || !sideNeedsPostSelectionCentering(side)) continue;
      const key = sideEndpointKey(nodeId, side);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push({ relationshipId, endpointIndex, rect, side, relationship });
    }
  }
  for (const endpoints of groups.values()) {
    const rect = endpoints[0].rect;
    const side = endpoints[0].side;
    const axis = side === "left" || side === "right" ? "y" : "x";
    const center = axis === "y" ? rect.y + rect.height / 2 : rect.x + rect.width / 2;
    const oppositeCenterOf = (endpoint) => {
      const oppositeNodeId = endpoint.endpointIndex === 0 ? endpoint.relationship.to : endpoint.relationship.from;
      const oppositeRect = input.nodeRects.get(oppositeNodeId);
      if (!oppositeRect) return 0;
      return axis === "y" ? oppositeRect.y + oppositeRect.height / 2 : oppositeRect.x + oppositeRect.width / 2;
    };
    const mountOf = (endpoint) => {
      const route = routeById.get(endpoint.relationshipId);
      const point = endpoint.endpointIndex === 0 ? route.points[0] : route.points.at(-1);
      return point[axis];
    };
    // Bundle reciprocal pairs (A->B and B->A both mounting this face) into one unit.
    const byNodePair = new Map();
    for (const endpoint of endpoints) {
      const pairKey = [endpoint.relationship.from, endpoint.relationship.to].sort().join(" ");
      if (!byNodePair.has(pairKey)) byNodePair.set(pairKey, []);
      byNodePair.get(pairKey).push(endpoint);
    }
    const units = [];
    for (const members of byNodePair.values()) {
      const reciprocal = members.length === 2 &&
        members[0].relationship.from === members[1].relationship.to &&
        members[0].relationship.to === members[1].relationship.from;
      if (reciprocal) {
        units.push({ members, oppositeCenter: oppositeCenterOf(members[0]) });
      } else {
        for (const member of members) units.push({ members: [member], oppositeCenter: oppositeCenterOf(member) });
      }
    }
    // Every populated face is re-distributed, including lone mounts (one unit -> offset 0 ->
    // centred). recenterSingletonSideEndpoints centres singletons too, but it runs BEFORE relief
    // and the mount optimizer, which then drag the mount back off-centre; running here (last) is
    // what actually makes lone-mount centring stick. The per-face guard below reverts any move
    // that would add a bend, a node collision, or a shared segment, so a mount that is off-centre
    // only because centring it would kink the edge is correctly left alone.
    if (units.length === 0) continue;
    units.sort((left, right) =>
      left.oppositeCenter - right.oppositeCenter ||
      (left.members[0].relationship.displayIndex ?? 0) - (right.members[0].relationship.displayIndex ?? 0) ||
      left.members[0].relationshipId.localeCompare(right.members[0].relationshipId));

    const affected = [...new Set(endpoints.map((endpoint) => endpoint.relationshipId))];
    keepMountMovesUnlessWorse(routeById, relationshipById, input, affected, () => {
      units.forEach((unit, index) => {
        const targetOffset = endpointSpreadOffset(index, units.length, rect, side);
        // Keep a pair's internal spacing (its parallel gap) by translating both mounts to
        // the unit's slot; a single member just lands on the slot centre.
        const unitCenter = unit.members.reduce((sum, member) => sum + mountOf(member), 0) / unit.members.length;
        // Leave a unit that already sits on its slot untouched, so an already-even face stays
        // byte-identical (no spurious collinear waypoints from re-anchoring an unchanged mount).
        if (Math.abs(unitCenter - (center + targetOffset)) < 0.5) return;
        for (const member of unit.members) {
          const memberOffset = mountOf(member) - unitCenter + targetOffset;
          const route = routeById.get(member.relationshipId);
          routeById.set(member.relationshipId, offsetEndpointRoute(route, member.endpointIndex, rect, side, memberOffset));
        }
      });
    });
  }
}

// Final pair-aware ordering pass (pair-internal first), run last so it has the final say. A
// reciprocal pair should render as two parallel lines; when its two routes cross each other — a
// misaligned/jogged facing pair the per-face distribution leaves behind (e.g. model-inference
// route-cloud vs cloud-provider-result, mounted at {1033,1045} on one face and {1010,1022} on the
// other, so each line jogs and the jogs cross) — rebuild BOTH as straight parallel runs. Only a
// pair on a directly-facing surface-pair is handled (a single straight vertical top<->bottom or
// horizontal left<->right run must be valid for both lines); detour/perpendicular pairs are left
// alone. Each line is straightened to its mount coordinate on the MORE-occupied (hub) face, so the
// hub face's distribution is preserved and only the lighter face's end moves; a coincident pair is
// split to a parallel gap. Guarded: kept only if it strictly reduces the crossings touching the
// pair and adds no node collision or shared segment.
function straightenSelfCrossingPairs(routeById, relationshipById, input) {
  const relationships = [...relationshipById.values()].filter((relationship) => relationship.relationshipType === "flow");
  const faceOccupancy = new Map();
  for (const [relationshipId, route] of routeById) {
    const relationship = relationshipById.get(relationshipId);
    if (!relationship || relationship.relationshipType !== "flow" || !route.points?.length) continue;
    for (const [nodeId, point] of [[relationship.from, route.points[0]], [relationship.to, route.points.at(-1)]]) {
      const rect = input.nodeRects.get(nodeId);
      const side = rect ? endpointSide(rect, point) : "";
      if (!side) continue;
      faceOccupancy.set(sideEndpointKey(nodeId, side), (faceOccupancy.get(sideEndpointKey(nodeId, side)) ?? 0) + 1);
    }
  }
  const crossingsTouching = (ids) => {
    const idSet = new Set(ids);
    const entries = [...routeById.entries()];
    let total = 0;
    for (let i = 0; i < entries.length; i += 1) {
      for (let j = i + 1; j < entries.length; j += 1) {
        if (!idSet.has(entries[i][0]) && !idSet.has(entries[j][0])) continue;
        total += crossingsBetween(entries[i][1], entries[j][1]);
      }
    }
    return total;
  };
  for (const [idA, idB] of reciprocalPairsByAdjacency(relationships)) {
    const routeA = routeById.get(idA);
    const routeB = routeById.get(idB);
    if (!routeA || !routeB || crossingsBetween(routeA, routeB) === 0) continue;
    const relationship = relationshipById.get(idA);
    const fromRect = input.nodeRects.get(relationship.from);
    const toRect = input.nodeRects.get(relationship.to);
    if (!fromRect || !toRect || fromRect.fixedPorts || toRect.fixedPorts) continue;
    const fromSide = endpointSide(fromRect, routeA.points[0]);
    const toSide = endpointSide(toRect, routeA.points.at(-1));
    const vertical = (fromSide === "top" || fromSide === "bottom") && (toSide === "top" || toSide === "bottom");
    const horizontal = (fromSide === "left" || fromSide === "right") && (toSide === "left" || toSide === "right");
    if (!vertical && !horizontal) continue;
    const axis = vertical ? "x" : "y"; // the coordinate held constant along a straight run
    const lo = Math.max(axis === "x" ? fromRect.x : fromRect.y, axis === "x" ? toRect.x : toRect.y);
    const hi = Math.min(
      axis === "x" ? fromRect.x + fromRect.width : fromRect.y + fromRect.height,
      axis === "x" ? toRect.x + toRect.width : toRect.y + toRect.height
    );
    if (hi - lo < RECIPROCAL_PARALLEL_OFFSET) continue; // no room for two parallel lanes
    const anchorFrom = (faceOccupancy.get(sideEndpointKey(relationship.from, fromSide)) ?? 0) >=
      (faceOccupancy.get(sideEndpointKey(relationship.to, toSide)) ?? 0);
    const anchorNode = anchorFrom ? relationship.from : relationship.to;
    const clamp = (value) => Math.min(hi, Math.max(lo, value));
    const coordOnAnchor = (id) => {
      const route = routeById.get(id);
      const point = relationshipById.get(id).from === anchorNode ? route.points[0] : route.points.at(-1);
      return clamp(point[axis]);
    };
    let coordA = coordOnAnchor(idA);
    let coordB = coordOnAnchor(idB);
    if (Math.abs(coordA - coordB) < RECIPROCAL_PARALLEL_OFFSET) {
      const mid = clamp((coordA + coordB) / 2);
      const half = RECIPROCAL_PARALLEL_OFFSET / 2;
      const aFirst = coordA <= coordB;
      coordA = clamp(mid + (aFirst ? -half : half));
      coordB = clamp(mid + (aFirst ? half : -half));
    }
    const straighten = (id, coord) => {
      const route = routeById.get(id);
      const start = route.points[0];
      const end = route.points.at(-1);
      const points = vertical
        ? [{ x: coord, y: start.y }, { x: coord, y: end.y }]
        : [{ x: start.x, y: coord }, { x: end.x, y: coord }];
      routeById.set(id, routeWithPoints(route, points, route.controls));
    };
    const ids = [idA, idB];
    const saved = ids.map((id) => [id, routeById.get(id)]);
    const beforeCrossings = crossingsTouching(ids);
    const beforeShared = sharedSegmentCountInvolving(routeById, ids);
    straighten(idA, coordA);
    straighten(idB, coordB);
    const collides = ids.some((id) => routeCollidesWithNonEndpoints(routeById.get(id), relationshipById.get(id), input));
    if (collides || crossingsTouching(ids) >= beforeCrossings || sharedSegmentCountInvolving(routeById, ids) > beforeShared) {
      for (const [id, route] of saved) routeById.set(id, route);
    }
  }
}

function crossingsBetween(routeA, routeB) {
  const segmentsA = axisAlignedSegments(routeA);
  const segmentsB = axisAlignedSegments(routeB);
  // Inclusive bounds so T-junctions / touches (a corner landing on another edge) count
  // as intersections, not just strict "X" straddles. Shared mounts (both routes
  // terminating at the same point — a legitimate convergence) are excluded.
  const terminalA = new Set([`${routeA.points[0].x},${routeA.points[0].y}`, `${routeA.points.at(-1).x},${routeA.points.at(-1).y}`]);
  const terminalB = new Set([`${routeB.points[0].x},${routeB.points[0].y}`, `${routeB.points.at(-1).x},${routeB.points.at(-1).y}`]);
  const points = new Set();
  for (const left of segmentsA) {
    for (const right of segmentsB) {
      if (left.orientation === right.orientation) continue;
      const horizontal = left.orientation === "horizontal" ? left : right;
      const vertical = left.orientation === "horizontal" ? right : left;
      if (
        vertical.x >= horizontal.min && vertical.x <= horizontal.max &&
        horizontal.y >= vertical.min && horizontal.y <= vertical.max
      ) {
        const key = `${vertical.x},${horizontal.y}`;
        if (terminalA.has(key) && terminalB.has(key)) continue; // shared mount
        points.add(key);
      }
    }
  }
  return points.size;
}

// Local-search (bubble-sort) crossing reduction: when two edges cross AND share a
// mount surface, try swapping their mount offsets on that surface; keep the swap
// only if it reduces total crossings without colliding with a node. Bounded passes.
function reduceCrossingsBySurfaceSwaps(routeById, relationshipById, input) {
  const ids = [...routeById.keys()];
  if (ids.length < 2 || ids.length > 80) return;
  const totalCrossings = () => {
    let total = 0;
    for (let i = 0; i < ids.length; i += 1) {
      for (let j = i + 1; j < ids.length; j += 1) {
        total += crossingsBetween(routeById.get(ids[i]), routeById.get(ids[j]));
      }
    }
    return total;
  };
  const surfaceEndpoints = (relationshipId) => {
    const relationship = relationshipById.get(relationshipId);
    const route = routeById.get(relationshipId);
    if (!relationship || !route?.points?.length) return [];
    const out = [];
    const fromRect = input.nodeRects.get(relationship.from);
    const toRect = input.nodeRects.get(relationship.to);
    const startSide = fromRect ? endpointSide(fromRect, route.points[0]) : "";
    const endSide = toRect ? endpointSide(toRect, route.points.at(-1)) : "";
    if (fromRect && !fromRect.fixedPorts && sideNeedsPostSelectionCentering(startSide)) {
      out.push({ node: relationship.from, side: startSide, endpointIndex: 0, rect: fromRect });
    }
    if (toRect && !toRect.fixedPorts && sideNeedsPostSelectionCentering(endSide)) {
      out.push({ node: relationship.to, side: endSide, endpointIndex: route.points.length - 1, rect: toRect });
    }
    return out;
  };
  let improved = true;
  for (let pass = 0; pass < 12 && improved; pass += 1) {
    improved = false;
    for (let i = 0; i < ids.length; i += 1) {
      for (let j = i + 1; j < ids.length; j += 1) {
        const a = ids[i];
        const b = ids[j];
        if (crossingsBetween(routeById.get(a), routeById.get(b)) === 0) continue;
        for (const pa of surfaceEndpoints(a)) {
          for (const pb of surfaceEndpoints(b)) {
            if (pa.node !== pb.node || pa.side !== pb.side) continue;
            const routeA = routeById.get(a);
            const routeB = routeById.get(b);
            const axis = pa.side === "left" || pa.side === "right" ? "y" : "x";
            const center = axis === "y" ? pa.rect.y + pa.rect.height / 2 : pa.rect.x + pa.rect.width / 2;
            const pointA = pa.endpointIndex === 0 ? routeA.points[0] : routeA.points.at(-1);
            const pointB = pb.endpointIndex === 0 ? routeB.points[0] : routeB.points.at(-1);
            const offsetA = pointA[axis] - center;
            const offsetB = pointB[axis] - center;
            if (Math.abs(offsetA - offsetB) < 0.5) continue;
            const before = totalCrossings();
            const swappedA = offsetEndpointRoute(routeA, pa.endpointIndex, pa.rect, pa.side, offsetB);
            const swappedB = offsetEndpointRoute(routeB, pb.endpointIndex, pb.rect, pb.side, offsetA);
            routeById.set(a, swappedA);
            routeById.set(b, swappedB);
            const collides =
              routeCollidesWithNonEndpoints(swappedA, relationshipById.get(a), input) ||
              routeCollidesWithNonEndpoints(swappedB, relationshipById.get(b), input);
            if (!collides && totalCrossings() < before) {
              improved = true;
            } else {
              routeById.set(a, routeA);
              routeById.set(b, routeB);
            }
          }
        }
      }
    }
  }
}

// Offset an axis-aligned polyline perpendicular to each segment by `delta`
// (consistent winding), reconnecting at the shifted right-angle corners. Endpoints
// shift ALONG their node surface (the first/last segment is the perpendicular stub),
// so the mount stays on the same surface at a parallel offset.
function offsetOrthogonalPolyline(points, delta) {
  if (!points || points.length < 2) return points;
  const segments = [];
  for (let index = 0; index < points.length - 1; index += 1) {
    const a = points[index];
    const b = points[index + 1];
    const dirX = Math.sign(b.x - a.x);
    const dirY = Math.sign(b.y - a.y);
    const normalX = dirY;
    const normalY = -dirX;
    segments.push({
      a: { x: a.x + normalX * delta, y: a.y + normalY * delta },
      b: { x: b.x + normalX * delta, y: b.y + normalY * delta },
      vertical: a.x === b.x
    });
  }
  const out = [{ ...segments[0].a }];
  for (let index = 0; index < segments.length - 1; index += 1) {
    const current = segments[index];
    const next = segments[index + 1];
    const x = current.vertical ? current.a.x : next.a.x;
    const y = current.vertical ? next.a.y : current.a.y;
    out.push({ x, y });
  }
  out.push({ ...segments[segments.length - 1].b });
  return out;
}

// A reciprocal pair (A->B and B->A) can only avoid crossing if the return runs
// parallel to the request. Route the return as a constant perpendicular offset of
// the request so the two never cross; keep the original return if the parallel
// version collides with a node.
function routeReciprocalPairsParallel(routeById, relationshipById, input, restrictIds = null) {
  const PARALLEL_OFFSET = 12;
  const byNodePair = new Map();
  for (const relationship of relationshipById.values()) {
    if (relationship.relationshipType !== "flow") continue;
    if (!routeById.has(relationship.id)) continue;
    const key = [relationship.from, relationship.to].sort().join("\0");
    if (!byNodePair.has(key)) byNodePair.set(key, []);
    byNodePair.get(key).push(relationship);
  }
  for (const group of byNodePair.values()) {
    if (group.length !== 2) continue;
    const [a, b] = group;
    if (a.from !== b.to || a.to !== b.from) continue;
    // When restricted (the post-relief re-parallel), only re-separate pairs relief just
    // relocated; pairs the main passes already laid out keep their existing separation.
    if (restrictIds && !restrictIds.has(a.id) && !restrictIds.has(b.id)) continue;
    const request = (a.displayIndex ?? 0) <= (b.displayIndex ?? 0) ? a : b;
    const ret = request === a ? b : a;
    const requestRoute = routeById.get(request.id);
    const returnRoute = routeById.get(ret.id);
    if (!requestRoute?.points?.length || !returnRoute?.points?.length) continue;
    const reversed = [...requestRoute.points].reverse();
    for (const delta of [PARALLEL_OFFSET, -PARALLEL_OFFSET]) {
      const candidatePoints = offsetOrthogonalPolyline(reversed, delta);
      const candidate = routeWithPoints(returnRoute, candidatePoints);
      if (!routeCollidesWithNonEndpoints(candidate, ret, input)) {
        routeById.set(ret.id, candidate);
        break;
      }
    }
  }
}

// Post-pass: after surfaces and cleanup are settled, re-spread each shared mount
// surface so the order of mount points along the surface matches the order of
// their opposite endpoints. Mount order that disagrees with destination order
// forces a crossing; sorting by the opposite endpoint's final position removes
// those same-surface crossings. Runs a couple of passes to let re-spreads on one
// surface settle the ordering on the surfaces they connect to.
function reorderSharedSurfaceMounts(routeById, relationshipById, input) {
  const axisFor = (side) => (side === "left" || side === "right" ? "y" : "x");
  for (let pass = 0; pass < 2; pass += 1) {
    const groups = new Map();
    for (const [relationshipId, route] of routeById) {
      const relationship = relationshipById.get(relationshipId);
      if (!relationship || relationship.relationshipType !== "flow" || !route.points?.length) continue;
      const register = (nodeId, endpointIndex) => {
        const rect = input.nodeRects.get(nodeId);
        const point = endpointIndex === 0 ? route.points[0] : route.points.at(-1);
        const side = rect ? endpointSide(rect, point) : "";
        if (!rect || rect.fixedPorts || !sideNeedsPostSelectionCentering(side)) return;
        const key = sideEndpointKey(nodeId, side);
        if (!groups.has(key)) groups.set(key, []);
        groups.get(key).push({ relationshipId, endpointIndex, rect, side });
      };
      register(relationship.from, 0);
      register(relationship.to, route.points.length - 1);
    }
    let changed = false;
    for (const endpoints of groups.values()) {
      if (endpoints.length < 2) continue;
      const axis = axisFor(endpoints[0].side);
      const enriched = endpoints.map((endpoint) => {
        const route = routeById.get(endpoint.relationshipId);
        const relationship = relationshipById.get(endpoint.relationshipId);
        const mountPoint = endpoint.endpointIndex === 0 ? route.points[0] : route.points.at(-1);
        const oppositeNodeId = endpoint.endpointIndex === 0 ? relationship?.to : relationship?.from;
        const oppositeRect = oppositeNodeId ? input.nodeRects.get(oppositeNodeId) : null;
        // Order by the opposite NODE's centre (stable and non-circular), then by a
        // stable key. Sorting by the opposite endpoint's live position is circular
        // for a request/return pair between the same two nodes — each surface would
        // sort by the other's positions and disagree, swapping the pair into a cross.
        const oppositeCenter = oppositeRect
          ? (axis === "y" ? oppositeRect.y + oppositeRect.height / 2 : oppositeRect.x + oppositeRect.width / 2)
          : 0;
        return { ...endpoint, mount: mountPoint[axis], oppositeCenter, displayIndex: relationship?.displayIndex ?? 0 };
      });
      const order = (list) => list.map((endpoint) => endpoint.relationshipId).join("|");
      const desired = [...enriched].sort((a, b) =>
        a.oppositeCenter - b.oppositeCenter || a.displayIndex - b.displayIndex || a.relationshipId.localeCompare(b.relationshipId));
      const current = [...enriched].sort((a, b) => a.mount - b.mount || a.relationshipId.localeCompare(b.relationshipId));
      if (order(desired) === order(current)) continue;
      desired.forEach((endpoint, index) => {
        const route = routeById.get(endpoint.relationshipId);
        const offset = endpointSpreadOffset(index, desired.length, endpoint.rect, endpoint.side);
        routeById.set(
          endpoint.relationshipId,
          offsetEndpointRoute(route, endpoint.endpointIndex, endpoint.rect, endpoint.side, offset)
        );
      });
      changed = true;
    }
    if (!changed) break;
  }
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
      route = recenteredWithoutNewSharedSegments(route, nextRoute, otherRoutes, relationship, input);
      points = route.points;
    }
    const toRect = input.nodeRects.get(relationship.to);
    const endSide = toRect ? endpointSide(toRect, points.at(-1)) : "";
    if (toRect && sideNeedsPostSelectionCentering(endSide) && endpointCounts.get(sideEndpointKey(relationship.to, endSide)) === 1) {
      const nextRoute = recenteredEndpointRoute(route, points.length - 1, toRect, endSide);
      const otherRoutes = [...routeById].filter(([otherRelationshipId]) => otherRelationshipId !== relationshipId).map(([, otherRoute]) => otherRoute);
      route = recenteredWithoutNewSharedSegments(route, nextRoute, otherRoutes, relationship, input);
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
  const allNodeRects = [...(input.nodeRects?.values?.() ?? [])];
  const nodeBounds = allNodeRects.length
    ? {
        minX: Math.min(...allNodeRects.map((rect) => rect.x)),
        minY: Math.min(...allNodeRects.map((rect) => rect.y)),
        maxX: Math.max(...allNodeRects.map((rect) => rect.x + rect.width)),
        maxY: Math.max(...allNodeRects.map((rect) => rect.y + rect.height))
      }
    : null;
  const routeCandidates = createRouteCandidateFactory({
    blockerRects,
    canvasHeight: input.canvasHeight,
    canvasWidth: input.canvasWidth,
    nodeBounds,
    gridRouteMaxExpansions: input.gridRouteMaxExpansions,
    gridRouteMaxPoints: input.gridRouteMaxPoints,
    rectFor,
    routeQualityFromSamples,
    stats
  });

  const edgePath = (relationship, index, pairIndex, usedRoutes, previousRoutes, routeIndex, endpointOffsets, endpointSideUsage, style = "orthogonal") => {
    const { from: fromId, to: toId } = relationship;
    const fromRect = rectFor(fromId);
    const toRect = rectFor(toId);
    const includeExteriorCorridors = Boolean(
      relationship.relationshipType === "flow" ||
      relationship.kind ||
      relationship.returnOf ||
      relationship.outcome ||
      relationship.stepId ||
      relationship.flowId
    );
    const corridors = [
      ...edgeCorridors(fromRect, toRect, diagramCorridors, { includeExterior: includeExteriorCorridors }),
      ...routeIndex.adjacentCorridors(fromRect, toRect)
    ];
    return selectRouteCandidate({
      collisionCount,
      corridors,
      endpointOffsets,
      endpointSideUsage,
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
      canvasWidth: input.canvasWidth,
      canvasHeight: input.canvasHeight,
      blockerRects: blockerRects(fromId, toId),
      fromLaneIndex: input.laneIndexByNode.get(fromId),
      toLaneIndex: input.laneIndexByNode.get(toId),
      fromRowIndex: input.rowIndexByNode.get(fromId),
      toRowIndex: input.rowIndexByNode.get(toId),
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
  const endpointSideUsage = createEndpointSideUsage();
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
        endpointSideUsage,
        style
      );
      plannedRawRoutes.push([relationship.id, route]);
      const fromRect = input.nodeRects.get(relationship.from);
      const toRect = input.nodeRects.get(relationship.to);
      endpointSideUsage.mark(relationship.from, fromRect ? endpointSide(fromRect, route.points[0]) : "");
      endpointSideUsage.mark(relationship.to, toRect ? endpointSide(toRect, route.points.at(-1)) : "");
      usedRoutes.push(route.samples);
      rawRoutes.push(route);
      routeIndex.add(route, rawRoutes.length - 1);
    });
    setCachedRawRoutes(cacheKey, plannedRawRoutes);
  }

  // Rebuild a single edge on forced mount sides for the relief pass. The route index is
  // populated from the relief pass's own current routes (not the cache-skipped planning
  // loop), so the rebuilt route is corridor-routed AROUND the other current routes while
  // planning stays deterministic across cache state. The reciprocal pair lands on the
  // shared gutter from this rebuild and is then nested by the crossing-reduction swap.
  // A dedicated planner is built only on the cache-hit path (planner is null there); the
  // common path reuses the planning-loop planner, so relief adds no extra precompute.
  const reliefPlanner = planner ?? routePlannerContext(input);
  const buildRouteForSides = (relationship, startSide, endSide, currentRoutes) => {
    const sideRouteIndex = createRouteIndex();
    if (currentRoutes) {
      let position = 0;
      for (const [otherId, otherRoute] of currentRoutes) {
        if (otherId === relationship.id) continue;
        sideRouteIndex.add(otherRoute, position);
        position += 1;
      }
    }
    const rebuilt = reliefPlanner.edgePath(
      { ...relationship, preferredStartSide: startSide, preferredEndSide: endSide },
      0,
      0,
      [],
      [],
      sideRouteIndex,
      { from: 0, to: 0 },
      createEndpointSideUsage(),
      style
    );
    // Relief runs after the global stub-enforcement pass, so each rebuilt route must
    // carry its own perpendicular endpoint stubs instead of grazing the node boundary.
    return rebuilt ? routeWithEndpointStubs(rebuilt, relationship, input) : rebuilt;
  };
  const endpointAdjustedRoutes = enforceEndpointStubs(
    spreadSharedSideEndpoints(recenterSingletonSideEndpoints(plannedRawRoutes, input), input),
    input
  );
  const separatedRoutes = separateCloseParallelRoutes(endpointAdjustedRoutes, input);
  // Relief runs LAST, as the final mount-assignment authority: it deliberately routes a
  // crowded reciprocal pair onto a parallel escape gutter, which the close-parallel
  // separation pass would otherwise tear apart, and spills over-capacity surfaces onto
  // empty perpendicular faces. Cost-guarded, so it can only improve or no-op.
  const relievedById = new Map(separatedRoutes);
  const relationshipById = new Map(input.relationships.map((relationship) => [relationship.id, relationship]));
  const relief = relieveCrowdedSurfaces(relievedById, relationshipById, input, buildRouteForSides);
  // After relief, replay the surface-cleanup passes in their original order so the relocated
  // routes are laid out as cleanly as the main passes lay out everything else:
  //   1. re-spread shared surfaces so a relief rebuild that landed an endpoint beside its
  //      neighbours is redistributed into the open slot (fixes bunched mounts);
  //   2. re-parallel — scoped to ONLY the pairs relief relocated onto a shared gutter — so
  //      both halves separate onto their own lanes (untouched pairs keep their separation);
  //   3. the crossing-reduction swap so the mounts order/nest without crossing.
  if (relief.anyMoved) {
    reorderSharedSurfaceMounts(relievedById, relationshipById, input);
    if (relief.pairs.length) {
      routeReciprocalPairsParallel(relievedById, relationshipById, input, new Set(relief.pairs.flat()));
    }
    reduceCrossingsBySurfaceSwaps(relievedById, relationshipById, input);
    //   4. re-align facing pairs. reorderSharedSurfaceMounts re-spreads each surface
    //      independently, so a facing reciprocal pair whose two ends sit on differently-
    //      populated surfaces (unified.right carries just the pair, memory.left also carries
    //      the sqlite traffic) lands on mismatched offsets and the straight run kinks. This is
    //      the same facing alignment spreadSharedSideEndpoints applies; the post-relief replay
    //      has to re-run it because reorder above just disturbed those offsets.
    realignFacingEndpoints(relievedById, relationshipById, input);
  }
  // Final mount-assignment authority: a cost-guarded refinement that re-homes endpoints to
  // the surfaces the lexicographic objective prefers (e.g. a reciprocal return mirroring its
  // request onto a like surface instead of hooking onto the facing side). Each move must
  // strictly improve a STRUCTURAL tier, so it can only refine the tuned-pass output.
  if (style === "orthogonal") {
    const preOptimizeRoutes = new Map(relievedById);
    optimizeMountAssignments(relievedById, relationshipById, input, { buildRouteForSides });
    // The cleanup below applies ONLY to edges the optimizer actually re-homed, so it never
    // disturbs the tuned-pass geometry of untouched edges (e.g. arrowhead-shifted mounts).
    const movedIds = new Set([...relievedById.keys()].filter((id) => relievedById.get(id) !== preOptimizeRoutes.get(id)));
    // A re-homed route must keep perpendicular endpoint contact. If a move left an edge
    // leaving its surface at an angle, restore that edge's tuned-pass route (which is
    // perpendicular by construction) — the refinement only keeps clean results.
    for (const relationshipId of movedIds) {
      const relationship = relationshipById.get(relationshipId);
      const route = relievedById.get(relationshipId);
      if (relationship && route && !routeEndpointsArePerpendicular(route, relationship, input)) {
        relievedById.set(relationshipId, preOptimizeRoutes.get(relationshipId));
      }
    }
  }
  // Final distribution authority: relief and the optimizer settle which FACE each endpoint
  // mounts; this evens out the SPACING within each face (reciprocal pairs kept parallel as a
  // single unit, lone mounts centred) so no face reads as crowded-at-one-end. Runs last so it
  // has the final word on distribution; guarded per face, so it only refines.
  distributeFacingReciprocalSurfaces(relievedById, relationshipById, input);
  distributeSurfaceMountUnits(relievedById, relationshipById, input);
  // Final pair-aware ordering: straighten any reciprocal pair the distribution passes left
  // crossing itself into two parallel runs. Runs last (after distribution) so distribution does
  // not re-jog it by moving the pair's two ends independently per face; guarded to crossings-only
  // wins, so it can only refine.
  if (style === "orthogonal") {
    straightenSelfCrossingPairs(relievedById, relationshipById, input);
  }
  const displayRawRoutes = separatedRoutes.map(([relationshipId]) => [relationshipId, relievedById.get(relationshipId)]);
  const routes = new Map();
  const allRawRoutes = displayRawRoutes.map(([, rawRoute]) => rawRoute);
  for (const [relationshipId, rawRoute] of displayRawRoutes) {
    const route = style === "orthogonal" ? renderOrthogonalRoute(rawRoute, allRawRoutes) : rawRoute;
    routes.set(relationshipId, route);
  }
  return routes;
}

export {
  endpointSide,
  crossingsBetween,
  axisAlignedSegments,
  sharedSegmentLength,
  sideNeedsPostSelectionCentering,
  routeCollidesWithNonEndpoints,
  routeHasEndpointTraversal,
  offsetEndpointRoute,
  endpointSpreadOffset,
  routeWithPoints,
  offsetOrthogonalPolyline
};
