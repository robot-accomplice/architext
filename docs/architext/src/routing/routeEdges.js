const SIDES = ["left", "right", "top", "bottom"];
const PORT_STUB = 18;
const PORT_SPACING = 6;
const HOP_RADIUS = 6;
const CORRIDOR_PADDING = 10;
const RAW_ROUTE_CACHE_LIMIT = 12;
const rawRouteCache = new Map();

export function anchorFor(rect, side) {
  if (side === "left") return { x: rect.x, y: rect.y + rect.height / 2 };
  if (side === "right") return { x: rect.x + rect.width, y: rect.y + rect.height / 2 };
  if (side === "top") return { x: rect.x + rect.width / 2, y: rect.y };
  return { x: rect.x + rect.width / 2, y: rect.y + rect.height };
}

export function sideVector(side) {
  if (side === "left") return { x: -1, y: 0 };
  if (side === "right") return { x: 1, y: 0 };
  if (side === "top") return { x: 0, y: -1 };
  return { x: 0, y: 1 };
}

function tangentVector(side) {
  return side === "left" || side === "right"
    ? { x: 0, y: 1 }
    : { x: 1, y: 0 };
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function offsetForEndpointOrder(order) {
  const lane = order % 7;
  const band = Math.floor(order / 7);
  return (lane - 3) * PORT_SPACING + band * PORT_SPACING * 7;
}

function portFor(rect, side, distance = PORT_STUB, rawOffset = 0) {
  const anchor = anchorFor(rect, side);
  const vector = sideVector(side);
  const maxOffset = (side === "left" || side === "right" ? rect.height : rect.width) / 2 - 8;
  const offset = clamp(rawOffset, -maxOffset, maxOffset);
  const tangent = tangentVector(side);
  const offsetAnchor = {
    x: anchor.x + tangent.x * offset,
    y: anchor.y + tangent.y * offset
  };
  return {
    anchor: offsetAnchor,
    port: {
      x: offsetAnchor.x + vector.x * distance,
      y: offsetAnchor.y + vector.y * distance
    }
  };
}

function portCandidatesFor(rect, side, offsets) {
  const maxOffset = (side === "left" || side === "right" ? rect.height : rect.width) / 2 - 8;
  return [...new Set(offsets.map((offset) => Math.round(clamp(offset, -maxOffset, maxOffset))))].map((offset) => portFor(rect, side, PORT_STUB, offset));
}

export function distanceToRect(point, rect) {
  const dx = Math.max(rect.x - point.x, 0, point.x - (rect.x + rect.width));
  const dy = Math.max(rect.y - point.y, 0, point.y - (rect.y + rect.height));
  return Math.hypot(dx, dy);
}

function distanceToRectSquared(point, rect) {
  const dx = Math.max(rect.x - point.x, 0, point.x - (rect.x + rect.width));
  const dy = Math.max(rect.y - point.y, 0, point.y - (rect.y + rect.height));
  return dx * dx + dy * dy;
}

export function lineSamples(points) {
  const samples = [];
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    for (let step = 1; step <= 10; step += 1) {
      const t = step / 10;
      samples.push({
        x: start.x + (end.x - start.x) * t,
        y: start.y + (end.y - start.y) * t
      });
    }
  }
  return samples;
}

function sampleLine(start, end, steps = 10) {
  const samples = [];
  for (let step = 1; step <= steps; step += 1) {
    const t = step / steps;
    samples.push({
      x: start.x + (end.x - start.x) * t,
      y: start.y + (end.y - start.y) * t
    });
  }
  return samples;
}

function quadraticPoint(start, control, end, t) {
  const inverse = 1 - t;
  return {
    x: inverse ** 2 * start.x + 2 * inverse * t * control.x + t ** 2 * end.x,
    y: inverse ** 2 * start.y + 2 * inverse * t * control.y + t ** 2 * end.y
  };
}

function sampleQuadratic(start, control, end, steps = 12) {
  const samples = [];
  for (let step = 1; step <= steps; step += 1) {
    samples.push(quadraticPoint(start, control, end, step / steps));
  }
  return samples;
}

export function nearestSample(samples, target) {
  return samples.reduce((nearest, sample) => {
    const nearestDistance = Math.hypot(nearest.x - target.x, nearest.y - target.y);
    const sampleDistance = Math.hypot(sample.x - target.x, sample.y - target.y);
    return sampleDistance < nearestDistance ? sample : nearest;
  }, samples[0] ?? target);
}

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

function rectDistance(a, b) {
  const dx = Math.max(a.x - (b.x + b.width), b.x - (a.x + a.width), 0);
  const dy = Math.max(a.y - (b.y + b.height), b.y - (a.y + a.height), 0);
  return Math.hypot(dx, dy);
}

function rectsOverlap(a, b, padding = 0) {
  return (
    a.x < b.x + b.width + padding &&
    a.x + a.width > b.x - padding &&
    a.y < b.y + b.height + padding &&
    a.y + a.height > b.y - padding
  );
}

function boundsForPoints(points) {
  if (!points.length) return { x: 0, y: 0, width: 0, height: 0 };
  let minX = points[0].x;
  let maxX = points[0].x;
  let minY = points[0].y;
  let maxY = points[0].y;
  for (let index = 1; index < points.length; index += 1) {
    const point = points[index];
    minX = Math.min(minX, point.x);
    maxX = Math.max(maxX, point.x);
    minY = Math.min(minY, point.y);
    maxY = Math.max(maxY, point.y);
  }
  return {
    x: minX,
    y: minY,
    width: maxX - minX,
    height: maxY - minY
  };
}

function estimatedLabelBox(labelPoint, relationship) {
  if (!relationship) return null;
  const text = relationship.label ?? relationship.id ?? "";
  const width = Math.max(24, Math.min(180, text.length * 6 + 12));
  const height = relationship.relationshipType === "flow" || relationship.stepId ? 24 : 18;
  return {
    x: labelPoint.x - width / 2,
    y: labelPoint.y - height / 2,
    width,
    height
  };
}

function uniqueRounded(values) {
  return [...new Set(values.map((value) => Math.round(value)))];
}

function createMinHeap() {
  const values = [];
  const swap = (left, right) => {
    const value = values[left];
    values[left] = values[right];
    values[right] = value;
  };
  const push = (item) => {
    values.push(item);
    let index = values.length - 1;
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (values[parent].distance <= values[index].distance) break;
      swap(parent, index);
      index = parent;
    }
  };
  const pop = () => {
    if (values.length === 0) return null;
    const root = values[0];
    const last = values.pop();
    if (values.length > 0) {
      values[0] = last;
      let index = 0;
      while (true) {
        const left = index * 2 + 1;
        const right = left + 1;
        let smallest = index;
        if (left < values.length && values[left].distance < values[smallest].distance) smallest = left;
        if (right < values.length && values[right].distance < values[smallest].distance) smallest = right;
        if (smallest === index) break;
        swap(index, smallest);
        index = smallest;
      }
    }
    return root;
  };
  return {
    get size() {
      return values.length;
    },
    push,
    pop
  };
}

function segmentIntersectsRect(start, end, rect, padding = 0) {
  const left = rect.x - padding;
  const right = rect.x + rect.width + padding;
  const top = rect.y - padding;
  const bottom = rect.y + rect.height + padding;
  const minX = Math.min(start.x, end.x);
  const maxX = Math.max(start.x, end.x);
  const minY = Math.min(start.y, end.y);
  const maxY = Math.max(start.y, end.y);

  if (start.y === end.y) {
    return start.y > top && start.y < bottom && maxX > left && minX < right;
  }
  if (start.x === end.x) {
    return start.x > left && start.x < right && maxY > top && minY < bottom;
  }
  return false;
}

function horizontalVerticalIntersection(horizontalStart, horizontalEnd, verticalStart, verticalEnd) {
  const minX = Math.min(horizontalStart.x, horizontalEnd.x);
  const maxX = Math.max(horizontalStart.x, horizontalEnd.x);
  const minY = Math.min(verticalStart.y, verticalEnd.y);
  const maxY = Math.max(verticalStart.y, verticalEnd.y);
  const x = verticalStart.x;
  const y = horizontalStart.y;
  if (x <= minX + HOP_RADIUS || x >= maxX - HOP_RADIUS || y <= minY + HOP_RADIUS || y >= maxY - HOP_RADIUS) {
    return null;
  }
  return { x, y };
}

function orthogonalCrossings(points, previousRoutes) {
  const crossings = new Map();
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    if (start.x !== end.x && start.y !== end.y) continue;

    for (const route of previousRoutes) {
      for (let usedIndex = 0; usedIndex < route.points.length - 1; usedIndex += 1) {
        const usedStart = route.points[usedIndex];
        const usedEnd = route.points[usedIndex + 1];
        if (usedStart.x !== usedEnd.x && usedStart.y !== usedEnd.y) continue;
        if (start.y === end.y && usedStart.x === usedEnd.x) {
          const crossing = horizontalVerticalIntersection(start, end, usedStart, usedEnd);
          if (crossing) {
            const direction = Math.sign(end.x - start.x);
            crossings.set(index, [...(crossings.get(index) ?? []), { ...crossing, direction }]);
          }
        } else if (start.x === end.x && usedStart.y === usedEnd.y) {
          const crossing = horizontalVerticalIntersection(usedStart, usedEnd, start, end);
          if (crossing) {
            const direction = Math.sign(end.y - start.y);
            crossings.set(index, [...(crossings.get(index) ?? []), { ...crossing, direction }]);
          }
        }
      }
    }
  }
  return crossings;
}

function orthogonalCrossingStats(points, previousRoutes) {
  const counts = new Map();
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    if (start.x !== end.x && start.y !== end.y) continue;

    previousRoutes.forEach((route, routeIndex) => {
      for (let usedIndex = 0; usedIndex < route.points.length - 1; usedIndex += 1) {
        const usedStart = route.points[usedIndex];
        const usedEnd = route.points[usedIndex + 1];
        if (usedStart.x !== usedEnd.x && usedStart.y !== usedEnd.y) continue;
        const crossing = start.y === end.y && usedStart.x === usedEnd.x
          ? horizontalVerticalIntersection(start, end, usedStart, usedEnd)
          : start.x === end.x && usedStart.y === usedEnd.y
            ? horizontalVerticalIntersection(usedStart, usedEnd, start, end)
            : null;
        if (crossing) counts.set(routeIndex, (counts.get(routeIndex) ?? 0) + 1);
      }
    });
  }

  const total = [...counts.values()].reduce((sum, count) => sum + count, 0);
  const repeated = [...counts.values()].reduce((sum, count) => sum + Math.max(0, count - 1), 0);
  return { total, repeated };
}

function createRouteIndex() {
  const horizontal = [];
  const vertical = [];
  const startPoints = new Set();
  const endPoints = new Set();

  const add = (route, routeIndex) => {
    if (!route?.points?.length) return;
    startPoints.add(`${route.points[0].x},${route.points[0].y}`);
    const last = route.points.at(-1);
    endPoints.add(`${last.x},${last.y}`);
    for (let index = 0; index < route.points.length - 1; index += 1) {
      const start = route.points[index];
      const end = route.points[index + 1];
      if (start.y === end.y) {
        horizontal.push({
          routeIndex,
          y: start.y,
          minX: Math.min(start.x, end.x),
          maxX: Math.max(start.x, end.x),
          start,
          end
        });
      } else if (start.x === end.x) {
        vertical.push({
          routeIndex,
          x: start.x,
          minY: Math.min(start.y, end.y),
          maxY: Math.max(start.y, end.y),
          start,
          end
        });
      }
    }
  };

  const crossingStats = (points) => {
    const counts = new Map();
    for (let index = 0; index < points.length - 1; index += 1) {
      const start = points[index];
      const end = points[index + 1];
      if (start.x !== end.x && start.y !== end.y) continue;

      if (start.y === end.y) {
        const minX = Math.min(start.x, end.x);
        const maxX = Math.max(start.x, end.x);
        for (const segment of vertical) {
          if (
            segment.x > minX + HOP_RADIUS &&
            segment.x < maxX - HOP_RADIUS &&
            start.y > segment.minY + HOP_RADIUS &&
            start.y < segment.maxY - HOP_RADIUS
          ) {
            counts.set(segment.routeIndex, (counts.get(segment.routeIndex) ?? 0) + 1);
          }
        }
      } else {
        const minY = Math.min(start.y, end.y);
        const maxY = Math.max(start.y, end.y);
        for (const segment of horizontal) {
          if (
            start.x > segment.minX + HOP_RADIUS &&
            start.x < segment.maxX - HOP_RADIUS &&
            segment.y > minY + HOP_RADIUS &&
            segment.y < maxY - HOP_RADIUS
          ) {
            counts.set(segment.routeIndex, (counts.get(segment.routeIndex) ?? 0) + 1);
          }
        }
      }
    }

    const total = [...counts.values()].reduce((sum, count) => sum + count, 0);
    const repeated = [...counts.values()].reduce((sum, count) => sum + Math.max(0, count - 1), 0);
    return { total, repeated };
  };

  const hasStackedEndpoint = (route) => {
    if (!route?.points?.length) return false;
    const start = route.points[0];
    const end = route.points.at(-1);
    return startPoints.has(`${start.x},${start.y}`) || endPoints.has(`${end.x},${end.y}`);
  };

  return { add, crossingStats, hasStackedEndpoint };
}

export function pathToSvgWithHops(points, previousRoutes) {
  const crossings = orthogonalCrossings(points, previousRoutes);
  if (crossings.size === 0) return pathToSvg(points);

  const commands = [`M ${points[0].x} ${points[0].y}`];
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    const segmentCrossings = (crossings.get(index) ?? []).sort((a, b) => (
      start.x === end.x
        ? Math.abs(a.y - start.y) - Math.abs(b.y - start.y)
        : Math.abs(a.x - start.x) - Math.abs(b.x - start.x)
    ));
    for (const crossing of segmentCrossings) {
      if (start.y === end.y) {
        const before = { x: crossing.x - crossing.direction * HOP_RADIUS, y: crossing.y };
        const after = { x: crossing.x + crossing.direction * HOP_RADIUS, y: crossing.y };
        commands.push(`L ${before.x} ${before.y}`);
        commands.push(`Q ${crossing.x} ${crossing.y - HOP_RADIUS * 1.6} ${after.x} ${after.y}`);
      } else {
        const before = { x: crossing.x, y: crossing.y - crossing.direction * HOP_RADIUS };
        const after = { x: crossing.x, y: crossing.y + crossing.direction * HOP_RADIUS };
        commands.push(`L ${before.x} ${before.y}`);
        commands.push(`Q ${crossing.x + HOP_RADIUS * 1.6} ${crossing.y} ${after.x} ${after.y}`);
      }
    }
    commands.push(`L ${end.x} ${end.y}`);
  }
  return commands.join(" ");
}

function simplifyOrthogonalPoints(points) {
  const deduped = [];
  for (const point of points) {
    const previous = deduped[deduped.length - 1];
    if (!previous || previous.x !== point.x || previous.y !== point.y) deduped.push(point);
  }

  const simplified = [];
  for (let index = 0; index < deduped.length; index += 1) {
    const point = deduped[index];
    const previous = simplified[simplified.length - 1];
    const beforePrevious = simplified[simplified.length - 2];
    if (
      index !== 2 &&
      index !== deduped.length - 1 &&
      previous &&
      beforePrevious &&
      ((beforePrevious.x === previous.x && previous.x === point.x) ||
        (beforePrevious.y === previous.y && previous.y === point.y))
    ) {
      simplified[simplified.length - 1] = point;
    } else {
      simplified.push(point);
    }
  }
  return simplified;
}

function pathToSvg(points) {
  return points.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x} ${point.y}`).join(" ");
}

function bendCount(points) {
  let bends = 0;
  for (let index = 1; index < points.length - 1; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const next = points[index + 1];
    if (
      (previous.x === current.x && current.x !== next.x) ||
      (previous.y === current.y && current.y !== next.y)
    ) {
      bends += 1;
    }
  }
  return bends;
}

function shallowJogCount(points) {
  let count = 0;
  for (let index = 1; index < points.length - 2; index += 1) {
    const before = points[index - 1];
    const start = points[index];
    const end = points[index + 1];
    const after = points[index + 2];
    const middleLength = Math.hypot(end.x - start.x, end.y - start.y);
    const horizontalJog = before.y === start.y && end.y === after.y && start.x === end.x;
    const verticalJog = before.x === start.x && end.x === after.x && start.y === end.y;
    if ((horizontalJog || verticalJog) && middleLength < 36) count += 1;
  }
  return count;
}

function routeLength(samples) {
  let length = 0;
  for (let index = 0; index < samples.length - 1; index += 1) {
    length += Math.hypot(samples[index + 1].x - samples[index].x, samples[index + 1].y - samples[index].y);
  }
  return length;
}

function totalQualityCost(qualityCosts) {
  return Object.values(qualityCosts).reduce((sum, value) => sum + value, 0);
}

function withQualityCosts(route, qualityCosts) {
  const normalizedQualityCosts = {
    lengthCost: 0,
    boundaryCost: 0,
    nodeClearanceCost: 0,
    edgeProximityCost: 0,
    labelNodeClearanceCost: 0,
    pointCountCost: 0,
    bendCost: 0,
    doglegCost: 0,
    perimeterFallbackCost: 0,
    perimeterLengthCost: 0,
    directnessReward: 0,
    crossingCost: 0,
    repeatedCrossingCost: 0,
    monotonicBacktrackCost: 0,
    fanOutDirectionCost: 0,
    endpointStackCost: 0,
    ...qualityCosts
  };
  return {
    ...route,
    qualityCosts: normalizedQualityCosts,
    cost: totalQualityCost(normalizedQualityCosts)
  };
}

function withReadableLabel(route) {
  const length = routeLength(route.samples);
  if (length >= 70) return route;

  const start = route.points[0];
  const isVertical = route.points.every((point) => point.x === start.x);
  const isHorizontal = route.points.every((point) => point.y === start.y);
  if (isVertical) {
    return { ...route, labelX: route.labelX + 28 };
  }
  if (isHorizontal) {
    return { ...route, labelY: route.labelY - 22 };
  }
  return route;
}

function monotonicBacktrackCost(points, fromRect, toRect) {
  const fromCenter = {
    x: fromRect.x + fromRect.width / 2,
    y: fromRect.y + fromRect.height / 2
  };
  const toCenter = {
    x: toRect.x + toRect.width / 2,
    y: toRect.y + toRect.height / 2
  };
  const xDirection = Math.sign(toCenter.x - fromCenter.x);
  const yDirection = Math.sign(toCenter.y - fromCenter.y);
  let cost = 0;
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    if (xDirection !== 0 && Math.sign(dx) === -xDirection) cost += Math.abs(dx) * 18;
    if (yDirection !== 0 && Math.sign(dy) === -yDirection) cost += Math.abs(dy) * 18;
  }
  return cost;
}

function roundedOrthogonalRoute(points, radius = 14) {
  if (points.length < 3) {
    return { d: pathToSvg(points), samples: lineSamples(points) };
  }

  const commands = [`M ${points[0].x} ${points[0].y}`];
  const samples = [];
  let cursor = points[0];
  for (let index = 1; index < points.length - 1; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const next = points[index + 1];
    const incomingLength = Math.hypot(current.x - previous.x, current.y - previous.y);
    const outgoingLength = Math.hypot(next.x - current.x, next.y - current.y);
    const bendRadius = Math.min(radius, incomingLength / 2, outgoingLength / 2);

    if (bendRadius <= 0 || incomingLength === 0 || outgoingLength === 0) {
      commands.push(`L ${current.x} ${current.y}`);
      samples.push(...sampleLine(cursor, current));
      cursor = current;
      continue;
    }

    const beforeBend = {
      x: current.x - ((current.x - previous.x) / incomingLength) * bendRadius,
      y: current.y - ((current.y - previous.y) / incomingLength) * bendRadius
    };
    const afterBend = {
      x: current.x + ((next.x - current.x) / outgoingLength) * bendRadius,
      y: current.y + ((next.y - current.y) / outgoingLength) * bendRadius
    };

    commands.push(`L ${beforeBend.x} ${beforeBend.y}`);
    commands.push(`Q ${current.x} ${current.y} ${afterBend.x} ${afterBend.y}`);
    samples.push(...sampleLine(cursor, beforeBend));
    samples.push(...sampleQuadratic(beforeBend, current, afterBend));
    cursor = afterBend;
  }
  const finalPoint = points[points.length - 1];
  commands.push(`L ${finalPoint.x} ${finalPoint.y}`);
  samples.push(...sampleLine(cursor, finalPoint));
  return { d: commands.join(" "), samples };
}

function renderRoute(route, style, previousRoutes) {
  if (style === "curved") {
    const rendered = roundedOrthogonalRoute(route.points);
    const label = nearestSample(rendered.samples, { x: route.labelX, y: route.labelY });
    return withReadableLabel({ ...route, ...rendered, sampleBounds: boundsForPoints(rendered.samples), labelX: label.x, labelY: label.y, style });
  }
  return withReadableLabel({ ...route, d: pathToSvgWithHops(route.points, previousRoutes), sampleBounds: boundsForPoints(route.samples), style: "orthogonal" });
}

function mapEntries(map) {
  return Array.from(map.entries()).sort(([left], [right]) => String(left).localeCompare(String(right)));
}

function routeCacheKey(input) {
  return JSON.stringify({
    relationships: input.relationships.map((relationship) => ({
      id: relationship.id,
      from: relationship.from,
      to: relationship.to,
      label: relationship.label,
      relationshipType: relationship.relationshipType,
      stepId: relationship.stepId,
      flowId: relationship.flowId
    })),
    visibleNodeIds: Array.from(input.visibleNodeIds).sort(),
    nodeRects: mapEntries(input.nodeRects),
    laneIndexByNode: mapEntries(input.laneIndexByNode),
    rowIndexByNode: mapEntries(input.rowIndexByNode),
    canvasWidth: input.canvasWidth,
    canvasHeight: input.canvasHeight,
    marginY: input.marginY,
    scoreEdgeProximity: Boolean(input.scoreEdgeProximity)
  });
}

function getCachedRawRoutes(key) {
  const cached = rawRouteCache.get(key);
  if (!cached) return null;
  rawRouteCache.delete(key);
  rawRouteCache.set(key, cached);
  return cached;
}

function setCachedRawRoutes(key, value) {
  rawRouteCache.set(key, value);
  while (rawRouteCache.size > RAW_ROUTE_CACHE_LIMIT) {
    rawRouteCache.delete(rawRouteCache.keys().next().value);
  }
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
      if (input.scoreEdgeProximity) {
        for (const usedRoute of usedRoutes) {
          for (let usedIndex = 0; usedIndex < usedRoute.length; usedIndex += 2) {
            const used = usedRoute[usedIndex];
            const distance = Math.hypot(point.x - used.x, point.y - used.y);
            if (distance < 26) qualityCosts.edgeProximityCost += 450;
            if (distance < 12) qualityCosts.edgeProximityCost += 1600;
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
      let collided = false;
      for (let index = 0; index < route.points.length - 1; index += 1) {
        if (segmentIntersectsRect(route.points[index], route.points[index + 1], rect, padding)) {
          collided = true;
          break;
        }
      }
      if (collided) {
        collisions += 1;
      }
    }
    return collisions;
  };

  const gridRoute = (relationship, fromId, toId, startSide, endSide, routeOffset, usedRoutes, startPort, endPort) => {
    if (stats) stats.gridRouteCalls = (stats.gridRouteCalls ?? 0) + 1;
    const start = startPort.port;
    const end = endPort.port;
    const fromRect = rectFor(fromId);
    const toRect = rectFor(toId);
    const blockers = blockerRects(fromId, toId);
    const padding = CORRIDOR_PADDING;
    const minX = 24;
    const maxX = input.canvasWidth - 24;
    const minY = 30;
    const maxY = input.canvasHeight - 24;
    const add = (set, value, min, max) => set.add(Math.min(max, Math.max(min, Math.round(value))));
    const xLines = new Set([Math.round(start.x), Math.round(end.x), minX, maxX]);
    const yLines = new Set([Math.round(start.y), Math.round(end.y), minY, maxY]);

    for (const rect of blockers) {
      add(xLines, rect.x - padding - routeOffset, minX, maxX);
      add(xLines, rect.x + rect.width + padding + routeOffset, minX, maxX);
      add(yLines, rect.y - padding - routeOffset, minY, maxY);
      add(yLines, rect.y + rect.height + padding + routeOffset, minY, maxY);
    }

    const xs = [...xLines].sort((a, b) => a - b);
    const ys = [...yLines].sort((a, b) => a - b);
    const points = [];
    const pointIndex = new Map();
    for (const x of xs) {
      for (const y of ys) {
        const key = `${x},${y}`;
        pointIndex.set(key, points.length);
        points.push({ x, y });
      }
    }

    const pointKey = (point) => `${Math.round(point.x)},${Math.round(point.y)}`;
    const startIndex = pointIndex.get(pointKey(start));
    const endIndex = pointIndex.get(pointKey(end));
    if (startIndex === undefined || endIndex === undefined) return null;

    const neighbors = Array.from({ length: points.length }, () => []);
    const horizontalBlockersByY = new Map(ys.map((y) => [
      y,
      blockers.filter((rect) => y > rect.y - padding && y < rect.y + rect.height + padding)
    ]));
    const verticalBlockersByX = new Map(xs.map((x) => [
      x,
      blockers.filter((rect) => x > rect.x - padding && x < rect.x + rect.width + padding)
    ]));
    const horizontalClear = (y, left, right) => {
      const minX = Math.min(left, right);
      const maxX = Math.max(left, right);
      return (horizontalBlockersByY.get(y) ?? []).every((rect) => maxX <= rect.x - padding || minX >= rect.x + rect.width + padding);
    };
    const verticalClear = (x, top, bottom) => {
      const minY = Math.min(top, bottom);
      const maxY = Math.max(top, bottom);
      return (verticalBlockersByX.get(x) ?? []).every((rect) => maxY <= rect.y - padding || minY >= rect.y + rect.height + padding);
    };

    for (const y of ys) {
      for (let index = 0; index < xs.length - 1; index += 1) {
        const a = pointIndex.get(`${xs[index]},${y}`);
        const b = pointIndex.get(`${xs[index + 1]},${y}`);
        if (horizontalClear(y, xs[index], xs[index + 1])) {
          const distance = Math.abs(xs[index + 1] - xs[index]);
          neighbors[a].push([b, distance]);
          neighbors[b].push([a, distance]);
        }
      }
    }
    for (const x of xs) {
      for (let index = 0; index < ys.length - 1; index += 1) {
        const a = pointIndex.get(`${x},${ys[index]}`);
        const b = pointIndex.get(`${x},${ys[index + 1]}`);
        if (verticalClear(x, ys[index], ys[index + 1])) {
          const distance = Math.abs(ys[index + 1] - ys[index]);
          neighbors[a].push([b, distance]);
          neighbors[b].push([a, distance]);
        }
      }
    }

    const distances = new Array(points.length).fill(Infinity);
    const previous = new Array(points.length).fill(-1);
    const visited = new Uint8Array(points.length);
    const queue = createMinHeap();
    distances[startIndex] = 0;
    queue.push({ index: startIndex, distance: 0 });

    while (queue.size > 0) {
      const nextItem = queue.pop();
      if (!nextItem || nextItem.distance !== distances[nextItem.index]) continue;
      const current = nextItem.index;
      if (current === endIndex) break;
      if (visited[current]) continue;
      visited[current] = 1;
      for (const [next, distance] of neighbors[current]) {
        if (visited[next]) continue;
        const turnPenalty = previous[current] >= 0
          ? (points[previous[current]].x !== points[current].x && points[current].x !== points[next].x) ||
            (points[previous[current]].y !== points[current].y && points[current].y !== points[next].y)
            ? 18
            : 0
          : 0;
        const nextDistance = distances[current] + distance + turnPenalty;
        if (nextDistance < distances[next]) {
          distances[next] = nextDistance;
          previous[next] = current;
          queue.push({ index: next, distance: nextDistance });
        }
      }
    }

    if (!Number.isFinite(distances[endIndex])) return null;

    const routePoints = [];
    for (let cursor = endIndex; cursor !== -1; cursor = previous[cursor]) {
      routePoints.unshift(points[cursor]);
    }
    const simplified = simplifyOrthogonalPoints([startPort.anchor, ...routePoints, endPort.anchor]);
    const samples = lineSamples(simplified);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2
    };
    const backtrackCost = monotonicBacktrackCost(simplified, fromRect, toRect);
    return withQualityCosts({
      d: pathToSvg(simplified),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(simplified),
      samples,
      points: simplified
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      pointCountCost: simplified.length * 24,
      bendCost: bendCount(simplified) * 420,
      doglegCost: shallowJogCount(simplified) * 14000,
      monotonicBacktrackCost: backtrackCost
    });
  };

  const perimeterRoute = (relationship, fromId, toId, side, routeOffset, usedRoutes, startPort, endPort) => {
    const start = startPort.port;
    const end = endPort.port;
    const gutter = side === "left"
      ? 24 + routeOffset
      : side === "right"
        ? input.canvasWidth - 24 - routeOffset
        : side === "top"
          ? 30 + routeOffset
          : input.canvasHeight - 24 - routeOffset;
    const points = side === "left" || side === "right"
      ? [
          startPort.anchor,
          start,
          { x: gutter, y: start.y },
          { x: gutter, y: end.y },
          end,
          endPort.anchor
        ]
      : [
          startPort.anchor,
          start,
          { x: start.x, y: gutter },
          { x: end.x, y: gutter },
          end,
          endPort.anchor
        ];
    const simplified = simplifyOrthogonalPoints(points);
    const samples = lineSamples(simplified);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2
    };
    return withQualityCosts({
      d: pathToSvg(simplified),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(simplified),
      samples,
      points: simplified
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      perimeterFallbackCost: 7000,
      perimeterLengthCost: routeLength(samples) * 8,
      pointCountCost: simplified.length * 24,
      bendCost: bendCount(simplified) * 420,
      doglegCost: shallowJogCount(simplified) * 14000
    });
  };

  const cornerPerimeterRoutes = (relationship, fromId, toId, routeOffset, usedRoutes, startPort, endPort) => {
    const boundaries = [
      { x: 24 + routeOffset, y: 30 + routeOffset },
      { x: input.canvasWidth - 24 - routeOffset, y: 30 + routeOffset },
      { x: 24 + routeOffset, y: input.canvasHeight - 24 - routeOffset },
      { x: input.canvasWidth - 24 - routeOffset, y: input.canvasHeight - 24 - routeOffset }
    ];

    const start = startPort.port;
    const end = endPort.port;
    return boundaries.flatMap((boundary) => [
      [
        startPort.anchor,
        start,
        { x: boundary.x, y: start.y },
        boundary,
        { x: boundary.x, y: end.y },
        end,
        endPort.anchor
      ],
      [
        startPort.anchor,
        start,
        { x: start.x, y: boundary.y },
        boundary,
        { x: end.x, y: boundary.y },
        end,
        endPort.anchor
      ]
    ]).map((points) => {
      const simplified = simplifyOrthogonalPoints(points);
      const samples = lineSamples(simplified);
      const start = simplified[0];
      const end = simplified[simplified.length - 1];
      const label = samples[Math.floor(samples.length / 2)] ?? {
        x: (start.x + end.x) / 2,
        y: (start.y + end.y) / 2
      };
      return withQualityCosts({
        d: pathToSvg(simplified),
        labelX: label.x,
        labelY: label.y,
        bends: bendCount(simplified),
        samples,
        points: simplified
      }, {
        ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
        perimeterFallbackCost: 12000,
        perimeterLengthCost: routeLength(samples) * 10,
        pointCountCost: simplified.length * 24,
        bendCost: bendCount(simplified) * 420,
        doglegCost: shallowJogCount(simplified) * 14000
      });
    });
  };

  const directPortCandidate = (relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort) => {
    const startVector = sideVector(startSide);
    const endVector = sideVector(endSide);
    const horizontal = startPort.port.y === endPort.port.y && startVector.y === 0 && endVector.y === 0;
    const vertical = startPort.port.x === endPort.port.x && startVector.x === 0 && endVector.x === 0;
    if (!horizontal && !vertical) return null;
    const points = simplifyOrthogonalPoints([startPort.anchor, startPort.port, endPort.port, endPort.anchor]);
    const samples = lineSamples(points);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (startPort.anchor.x + endPort.anchor.x) / 2,
      y: (startPort.anchor.y + endPort.anchor.y) / 2
    };
    return withQualityCosts({
      d: pathToSvg(points),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(points),
      samples,
      points
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      directnessReward: -2000,
      doglegCost: shallowJogCount(points) * 14000
    });
  };

  const corridorCandidate = (relationship, fromId, toId, usedRoutes, startPort, endPort, corridor) => {
    const start = startPort.port;
    const end = endPort.port;
    const points = corridor.axis === "x"
      ? [
          startPort.anchor,
          start,
          { x: corridor.value, y: start.y },
          { x: corridor.value, y: end.y },
          end,
          endPort.anchor
        ]
      : [
          startPort.anchor,
          start,
          { x: start.x, y: corridor.value },
          { x: end.x, y: corridor.value },
          end,
          endPort.anchor
    ];
    const simplified = simplifyOrthogonalPoints(points);
    const blockers = blockerRects(fromId, toId);
    if (!blockers.every((rect) => simplified.slice(0, -1).every((point, index) => !segmentIntersectsRect(point, simplified[index + 1], rect, CORRIDOR_PADDING)))) {
      return null;
    }
    const samples = lineSamples(simplified);
    const label = samples[Math.floor(samples.length / 2)] ?? {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2
    };
    return withQualityCosts({
      d: pathToSvg(simplified),
      labelX: label.x,
      labelY: label.y,
      bends: bendCount(simplified),
      samples,
      points: simplified
    }, {
      ...routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship),
      pointCountCost: simplified.length * 24,
      bendCost: bendCount(simplified) * 420,
      doglegCost: shallowJogCount(simplified) * 14000,
      monotonicBacktrackCost: monotonicBacktrackCost(simplified, rectFor(fromId), rectFor(toId))
    });
  };

  const interiorCorridors = (fromRect, toRect) => {
    const corridors = [];
    const verticalGapStart = Math.min(fromRect.y, toRect.y) + Math.min(fromRect.height, toRect.height);
    const verticalGapEnd = Math.max(fromRect.y, toRect.y);
    if (verticalGapEnd - verticalGapStart > PORT_STUB * 2) {
      corridors.push({ axis: "y", value: Math.round((verticalGapStart + verticalGapEnd) / 2) });
    }
    const horizontalGapStart = Math.min(fromRect.x, toRect.x) + Math.min(fromRect.width, toRect.width);
    const horizontalGapEnd = Math.max(fromRect.x, toRect.x);
    if (horizontalGapEnd - horizontalGapStart > PORT_STUB * 2) {
      corridors.push({ axis: "x", value: Math.round((horizontalGapStart + horizontalGapEnd) / 2) });
    }
    return corridors;
  };

  const mergeCorridors = (corridors) => {
    const seen = new Set();
    return corridors.filter((corridor) => {
      const key = `${corridor.axis}:${corridor.value}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  };

  const freeSpaceCorridors = () => {
    const minX = 24;
    const maxX = input.canvasWidth - 24;
    const minY = 30;
    const maxY = input.canvasHeight - 24;
    const verticalEdges = uniqueRounded(visibleRects.flatMap((rect) => [rect.x, rect.x + rect.width])).sort((a, b) => a - b);
    const horizontalEdges = uniqueRounded(visibleRects.flatMap((rect) => [rect.y, rect.y + rect.height])).sort((a, b) => a - b);
    const corridors = [];

    for (let index = 0; index < verticalEdges.length - 1; index += 1) {
      const left = verticalEdges[index];
      const right = verticalEdges[index + 1];
      if (right - left > CORRIDOR_PADDING * 3) {
        const value = Math.round((left + right) / 2);
        if (value > minX && value < maxX) corridors.push({ axis: "x", value });
      }
    }
    for (let index = 0; index < horizontalEdges.length - 1; index += 1) {
      const top = horizontalEdges[index];
      const bottom = horizontalEdges[index + 1];
      if (bottom - top > CORRIDOR_PADDING * 3) {
        const value = Math.round((top + bottom) / 2);
        if (value > minY && value < maxY) corridors.push({ axis: "y", value });
      }
    }
    return corridors;
  };

  const diagramCorridors = freeSpaceCorridors();

  const edgeCorridors = (fromRect, toRect) => {
    const minX = Math.min(fromRect.x, toRect.x) - PORT_STUB * 2;
    const maxX = Math.max(fromRect.x + fromRect.width, toRect.x + toRect.width) + PORT_STUB * 2;
    const minY = Math.min(fromRect.y, toRect.y) - PORT_STUB * 2;
    const maxY = Math.max(fromRect.y + fromRect.height, toRect.y + toRect.height) + PORT_STUB * 2;
    const midpoint = {
      x: (fromRect.x + fromRect.width / 2 + toRect.x + toRect.width / 2) / 2,
      y: (fromRect.y + fromRect.height / 2 + toRect.y + toRect.height / 2) / 2
    };
    const localCorridors = diagramCorridors.filter((corridor) => (
      corridor.axis === "x"
        ? corridor.value >= minX && corridor.value <= maxX
        : corridor.value >= minY && corridor.value <= maxY
    ));
    const closest = (axis) => localCorridors
      .filter((corridor) => corridor.axis === axis)
      .sort((left, right) => Math.abs(left.value - midpoint[axis]) - Math.abs(right.value - midpoint[axis]))
      .slice(0, 6);
    return mergeCorridors([
      ...interiorCorridors(fromRect, toRect),
      ...closest("x"),
      ...closest("y")
    ]);
  };

  const candidatePorts = (fromRect, toRect, startSide, endSide, endpointOffsets, scope = "cheap") => {
    const fromCenter = {
      x: fromRect.x + fromRect.width / 2,
      y: fromRect.y + fromRect.height / 2
    };
    const toCenter = {
      x: toRect.x + toRect.width / 2,
      y: toRect.y + toRect.height / 2
    };
    const startTangent = tangentVector(startSide);
    const endTangent = tangentVector(endSide);
    const targetAlignedStartOffset = startTangent.y !== 0 ? toCenter.y - fromCenter.y : toCenter.x - fromCenter.x;
    const targetAlignedEndOffset = endTangent.y !== 0 ? fromCenter.y - toCenter.y : fromCenter.x - toCenter.x;

    const sharedOffsets = scope === "grid"
      ? [
          0,
          endpointOffsets.from,
          endpointOffsets.to
        ]
      : [
      0,
      endpointOffsets.from,
      endpointOffsets.to,
      endpointOffsets.from + targetAlignedStartOffset,
      endpointOffsets.to + targetAlignedEndOffset
    ];

    return {
      starts: portCandidatesFor(fromRect, startSide, sharedOffsets),
      ends: portCandidatesFor(toRect, endSide, sharedOffsets)
    };
  };

  const sidePairsFor = (fromRect, toRect) => {
    const fromCenter = {
      x: fromRect.x + fromRect.width / 2,
      y: fromRect.y + fromRect.height / 2
    };
    const toCenter = {
      x: toRect.x + toRect.width / 2,
      y: toRect.y + toRect.height / 2
    };
    const horizontal = toCenter.x >= fromCenter.x ? ["right", "left"] : ["left", "right"];
    const vertical = toCenter.y >= fromCenter.y ? ["bottom", "top"] : ["top", "bottom"];
    const pairs = [
      Math.abs(toCenter.x - fromCenter.x) >= Math.abs(toCenter.y - fromCenter.y) ? horizontal : vertical,
      Math.abs(toCenter.x - fromCenter.x) >= Math.abs(toCenter.y - fromCenter.y) ? vertical : horizontal,
      ["left", "right"],
      ["right", "left"],
      ["top", "bottom"],
      ["bottom", "top"],
      ["left", "left"],
      ["right", "right"],
      ["top", "top"],
      ["bottom", "bottom"]
    ];
    const seen = new Set();
    return pairs.filter(([startSide, endSide]) => {
      const key = `${startSide}:${endSide}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  };

  const portPairsFor = (ports) => {
    const pairs = [];
    const seen = new Set();
    const add = (start, end) => {
      if (!start || !end) return;
      const key = `${start.anchor.x},${start.anchor.y}:${end.anchor.x},${end.anchor.y}`;
      if (seen.has(key)) return;
      seen.add(key);
      pairs.push([start, end]);
    };
    for (const start of ports.starts) {
      for (const end of ports.ends) {
        add(start, end);
      }
    }
    return pairs;
  };

  const edgePath = (relationship, index, pairIndex, usedRoutes, previousRoutes, routeIndex, endpointOffsets) => {
    const { from: fromId, to: toId } = relationship;
    const fromRect = rectFor(fromId);
    const toRect = rectFor(toId);
    const fromLane = input.laneIndexByNode.get(fromId) ?? 0;
    const toLane = input.laneIndexByNode.get(toId) ?? 0;
    const candidates = [];
    const candidateKeys = new Set();
    const addCandidate = (candidate) => {
      if (!candidate) return;
      const key = candidate.points.map((point) => `${point.x},${point.y}`).join("|");
      if (candidateKeys.has(key)) return;
      candidateKeys.add(key);
      candidates.push(candidate);
    };
    const corridors = edgeCorridors(fromRect, toRect);
    const sidePairs = sidePairsFor(fromRect, toRect);
    const cheapSidePairs = SIDES.flatMap((startSide) => SIDES.map((endSide) => [startSide, endSide]));
    const routeOffset = pairIndex * 40 + (index % 2) * 10;
    const topLimit = Math.min(fromRect.y, toRect.y);
    const bottomLimit = Math.max(fromRect.y + fromRect.height, toRect.y + toRect.height);

    const scoreCandidates = (candidateList) => {
      candidateList.forEach((candidate) => {
        const travelsTop = candidate.samples.some((point) => point.y < topLimit - 4);
        const travelsBottom = candidate.samples.some((point) => point.y > bottomLimit + 4);
        candidate.collisions = collisionCount(candidate, fromId, toId, 0);
        candidate.paddedCollisions = collisionCount(candidate, fromId, toId, 8);
        const crossingStats = routeIndex.crossingStats(candidate.points);
        candidate.crossings = crossingStats.total;
        candidate.repeatedCrossings = crossingStats.repeated;
        candidate.qualityCosts.crossingCost = crossingStats.total * 3000;
        candidate.qualityCosts.repeatedCrossingCost = crossingStats.repeated * 40000;
        candidate.qualityCosts.endpointStackCost = routeIndex.hasStackedEndpoint(candidate) ? 90000 : 0;
        if (pairIndex % 2 === 1 && travelsTop) {
          candidate.qualityCosts.fanOutDirectionCost = (candidate.qualityCosts.fanOutDirectionCost ?? 0) + 25000;
        }
        if (pairIndex % 2 === 1 && !travelsBottom) {
          candidate.qualityCosts.fanOutDirectionCost = (candidate.qualityCosts.fanOutDirectionCost ?? 0) + 4000;
        }
        if (pairIndex % 2 === 0 && travelsBottom) {
          candidate.qualityCosts.fanOutDirectionCost = (candidate.qualityCosts.fanOutDirectionCost ?? 0) + 600;
        }
        candidate.cost = totalQualityCost(candidate.qualityCosts);
      });
    };

    const sortedCandidates = (candidateList) => candidateList.sort((a, b) =>
      a.collisions - b.collisions ||
      a.paddedCollisions - b.paddedCollisions ||
      a.repeatedCrossings - b.repeatedCrossings ||
      a.crossings - b.crossings ||
      (a.qualityCosts.monotonicBacktrackCost > 0 ? 1 : 0) - (b.qualityCosts.monotonicBacktrackCost > 0 ? 1 : 0) ||
      (a.qualityCosts.endpointStackCost > 0 ? 1 : 0) - (b.qualityCosts.endpointStackCost > 0 ? 1 : 0) ||
      (a.qualityCosts.perimeterFallbackCost > 0 ? 1 : 0) - (b.qualityCosts.perimeterFallbackCost > 0 ? 1 : 0) ||
      a.bends - b.bends ||
      a.cost - b.cost
    );

    const cleanCandidate = (candidate) => (
      candidate.collisions === 0 &&
      candidate.paddedCollisions === 0 &&
      candidate.repeatedCrossings === 0 &&
      candidate.crossings === 0 &&
      candidate.qualityCosts.endpointStackCost === 0 &&
      candidate.qualityCosts.perimeterFallbackCost === 0 &&
      candidate.qualityCosts.doglegCost === 0
    );

    const cheapCandidates = [];
    const addCheapCandidate = (candidate) => {
      if (!candidate) return;
      const key = candidate.points.map((point) => `${point.x},${point.y}`).join("|");
      if (candidateKeys.has(key)) return;
      candidateKeys.add(key);
      cheapCandidates.push(candidate);
    };

    const addCheapCandidatesForSidePairs = (pairs) => {
      pairs.forEach(([startSide, endSide]) => {
        const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets);
        for (const [startPort, endPort] of portPairsFor(ports)) {
          const direct = directPortCandidate(relationship, fromId, toId, startSide, endSide, usedRoutes, startPort, endPort);
          addCheapCandidate(direct);
          for (const corridor of corridors) {
            const corridorRoute = corridorCandidate(relationship, fromId, toId, usedRoutes, startPort, endPort, corridor);
            addCheapCandidate(corridorRoute);
          }
        }
      });
    };

    addCheapCandidatesForSidePairs(cheapSidePairs);
    scoreCandidates(cheapCandidates);
    const hasCleanCheapCandidate = cheapCandidates.some(cleanCandidate);
    if (stats) {
      stats.edgesPlanned = (stats.edgesPlanned ?? 0) + 1;
      stats.cheapCandidateCount = (stats.cheapCandidateCount ?? 0) + cheapCandidates.length;
      if (!hasCleanCheapCandidate) {
        stats.gridEscalations = (stats.gridEscalations ?? 0) + 1;
        const bestCheap = sortedCandidates([...cheapCandidates])[0];
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
    if (hasCleanCheapCandidate) {
      candidates.push(...cheapCandidates);
    } else {
      candidates.push(...cheapCandidates);
      sidePairs.forEach(([startSide, endSide]) => {
        const ports = candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets, "grid");
        for (const [startPort, endPort] of portPairsFor(ports)) {
          const orthogonal = gridRoute(relationship, fromId, toId, startSide, endSide, routeOffset, usedRoutes, startPort, endPort);
          addCandidate(orthogonal);
        }
      });
    }

    if (!hasCleanCheapCandidate) {
      SIDES.forEach((side) => {
        const ports = candidatePorts(fromRect, toRect, side, side, endpointOffsets);
        for (const [startPort, endPort] of portPairsFor(ports)) {
          addCandidate(perimeterRoute(relationship, fromId, toId, side, routeOffset, usedRoutes, startPort, endPort));
          for (const perimeterCandidate of cornerPerimeterRoutes(relationship, fromId, toId, routeOffset, usedRoutes, startPort, endPort)) {
            addCandidate(perimeterCandidate);
          }
        }
      });
    }

    scoreCandidates(candidates.filter((candidate) => candidate.collisions === undefined));

    return sortedCandidates(candidates).map((candidate) => {
      const warnings = [];
      if (candidate.collisions > 0 || candidate.paddedCollisions > 0) {
        warnings.push({
          code: "least-bad-route",
          message: "No clean route was available for the current node arrangement."
        });
      }
      if (candidate.repeatedCrossings > 0) {
        warnings.push({
          code: "repeated-route-crossing",
          message: "Selected route crosses the same existing route more than once."
        });
      }
      if (candidate.qualityCosts.perimeterFallbackCost > 0) {
        warnings.push({
          code: "perimeter-fallback-route",
          message: "Selected route used a perimeter fallback instead of an interior corridor."
        });
      }
      if (rectDistance(fromRect, toRect) < PORT_STUB * 2) {
        warnings.push({
          code: "nodes-too-close",
          message: "Source and target nodes are too close for clean connector routing."
        });
      }
      return { ...candidate, warnings };
    })[0];
  };

  return { edgePath };
}

export function routeEdges(input) {
  const usedRoutes = [];
  const rawRoutes = [];
  const routeIndex = createRouteIndex();
  const pairCounts = new Map();
  const endpointCounts = new Map();
  const cacheKey = routeCacheKey(input);
  const cachedRawRoutes = getCachedRawRoutes(cacheKey);
  const plannedRawRoutes = cachedRawRoutes ?? [];
  const planner = cachedRawRoutes ? null : routePlannerContext(input);
  const style = input.style === "curved" ? "curved" : "orthogonal";

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
        }
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
    const route = renderRoute(rawRoute, style, renderedRoutes);
    routes.set(relationshipId, route);
    renderedRoutes.push(route);
  }
  return routes;
}
