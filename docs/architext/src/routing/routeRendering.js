export const HOP_RADIUS = 6;

export function horizontalVerticalIntersection(horizontalStart, horizontalEnd, verticalStart, verticalEnd) {
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

export function simplifyOrthogonalPoints(points) {
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

export function pathToSvg(points) {
  return points.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x} ${point.y}`).join(" ");
}
