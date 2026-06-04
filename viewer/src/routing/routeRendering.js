export const HOP_RADIUS = 6;
// Below this the hop arc is too small to read, so the crossing renders flat (a sub-pixel bump
// reads as a rendering glitch, not a hop). Every crossing with at least this much room is hopped.
const MIN_HOP_RADIUS = 2;

export function horizontalVerticalIntersection(horizontalStart, horizontalEnd, verticalStart, verticalEnd) {
  const minX = Math.min(horizontalStart.x, horizontalEnd.x);
  const maxX = Math.max(horizontalStart.x, horizontalEnd.x);
  const minY = Math.min(verticalStart.y, verticalEnd.y);
  const maxY = Math.max(verticalStart.y, verticalEnd.y);
  const x = verticalStart.x;
  const y = horizontalStart.y;
  if (x <= minX || x >= maxX || y <= minY || y >= maxY) {
    return null; // not an interior crossing
  }
  // Adaptive hop radius: a crossing close to a corner still gets a hop, just sized to the room
  // available before the nearest segment end (so the arc stays within both segments) instead of
  // being skipped. Only a crossing with less than MIN_HOP_RADIUS of room renders flat.
  const radius = Math.min(HOP_RADIUS, x - minX, maxX - x, y - minY, maxY - y);
  if (radius < MIN_HOP_RADIUS) {
    return null;
  }
  return { x, y, radius };
}

function routePoints(route) {
  return Array.isArray(route) ? route : route?.points;
}

function isSameRoute(points, route) {
  return route === points || route?.points === points;
}

function orthogonalCrossings(points, routes) {
  const crossings = new Map();
  for (let index = 0; index < points.length - 1; index += 1) {
    const start = points[index];
    const end = points[index + 1];
    if (start.x !== end.x && start.y !== end.y) continue;

    for (const route of routes) {
      if (isSameRoute(points, route)) continue;
      const otherPoints = routePoints(route);
      if (!otherPoints?.length) continue;
      for (let usedIndex = 0; usedIndex < otherPoints.length - 1; usedIndex += 1) {
        const usedStart = otherPoints[usedIndex];
        const usedEnd = otherPoints[usedIndex + 1];
        if (usedStart.x !== usedEnd.x && usedStart.y !== usedEnd.y) continue;
        if (start.x === end.x && usedStart.y === usedEnd.y) {
          const crossing = horizontalVerticalIntersection(usedStart, usedEnd, start, end);
          if (crossing) {
            const direction = Math.sign(end.y - start.y) || 1;
            crossings.set(index, [...(crossings.get(index) ?? []), { ...crossing, direction }]);
          }
        } else if (start.y === end.y && usedStart.x === usedEnd.x) {
          const crossing = horizontalVerticalIntersection(start, end, usedStart, usedEnd);
          if (crossing) {
            const direction = Math.sign(end.x - start.x) || 1;
            crossings.set(index, [...(crossings.get(index) ?? []), { ...crossing, direction }]);
          }
        }
      }
    }
  }
  return crossings;
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
      const radius = crossing.radius ?? HOP_RADIUS;
      if (start.y === end.y) {
        const before = { x: crossing.x - crossing.direction * radius, y: crossing.y };
        const after = { x: crossing.x + crossing.direction * radius, y: crossing.y };
        commands.push(`L ${before.x} ${before.y}`);
        commands.push(`Q ${crossing.x} ${crossing.y - radius * 1.6} ${after.x} ${after.y}`);
      } else {
        const before = { x: crossing.x, y: crossing.y - crossing.direction * radius };
        const after = { x: crossing.x, y: crossing.y + crossing.direction * radius };
        commands.push(`L ${before.x} ${before.y}`);
        commands.push(`Q ${crossing.x + radius * 1.6} ${crossing.y} ${after.x} ${after.y}`);
      }
    }
    commands.push(`L ${end.x} ${end.y}`);
  }
  return commands.join(" ");
}

export function simplifyOrthogonalPoints(points) {
  const portStubElbowIndex = 2;
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
      index !== portStubElbowIndex &&
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
  return collapseBacktrackingPoints(simplified);
}

function collapseBacktrackingPoints(points) {
  const collapsed = [...points];
  let changed = true;
  while (changed) {
    changed = false;
    for (let index = 1; index < collapsed.length - 1; index += 1) {
      const previous = collapsed[index - 1];
      const current = collapsed[index];
      const next = collapsed[index + 1];
      const horizontalBacktrack = previous.y === current.y && current.y === next.y &&
        Math.sign(current.x - previous.x) === -Math.sign(next.x - current.x);
      const verticalBacktrack = previous.x === current.x && current.x === next.x &&
        Math.sign(current.y - previous.y) === -Math.sign(next.y - current.y);
      if (horizontalBacktrack || verticalBacktrack) {
        collapsed.splice(index, 1);
        changed = true;
        break;
      }
    }
  }
  return collapsed;
}

export function pathToSvg(points) {
  return points.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x} ${point.y}`).join(" ");
}
