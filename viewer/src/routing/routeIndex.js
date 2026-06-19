import { HOP_RADIUS } from "./routeRendering.js";

export function createRouteIndex() {
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

  const sharedSegmentStats = (points) => {
    let count = 0;
    let length = 0;
    for (let index = 0; index < points.length - 1; index += 1) {
      const start = points[index];
      const end = points[index + 1];
      if (start.x !== end.x && start.y !== end.y) continue;

      if (start.y === end.y) {
        const minX = Math.min(start.x, end.x);
        const maxX = Math.max(start.x, end.x);
        for (const segment of horizontal) {
          if (segment.y !== start.y) continue;
          const overlap = Math.min(maxX, segment.maxX) - Math.max(minX, segment.minX);
          if (overlap > 1) {
            count += 1;
            length += overlap;
          }
        }
      } else {
        const minY = Math.min(start.y, end.y);
        const maxY = Math.max(start.y, end.y);
        for (const segment of vertical) {
          if (segment.x !== start.x) continue;
          const overlap = Math.min(maxY, segment.maxY) - Math.max(minY, segment.minY);
          if (overlap > 1) {
            count += 1;
            length += overlap;
          }
        }
      }
    }
    return { count, length };
  };

  const hasStackedEndpoint = (route) => {
    if (!route?.points?.length) return false;
    const start = route.points[0];
    const end = route.points.at(-1);
    return startPoints.has(`${start.x},${start.y}`) || endPoints.has(`${end.x},${end.y}`);
  };

  const adjacentCorridors = (fromRect, toRect, spacing = 12) => {
    const offsets = [1, 2, 3, 4].map((multiplier) => spacing * multiplier);
    const minX = Math.min(fromRect.x, toRect.x) - spacing * 6;
    const maxX = Math.max(fromRect.x + fromRect.width, toRect.x + toRect.width) + spacing * 6;
    const minY = Math.min(fromRect.y, toRect.y) - spacing * 6;
    const maxY = Math.max(fromRect.y + fromRect.height, toRect.y + toRect.height) + spacing * 6;
    const corridors = [];
    for (const segment of vertical) {
      if (segment.maxY < minY || segment.minY > maxY) continue;
      for (const offset of offsets) {
        for (const value of [segment.x - offset, segment.x + offset]) {
          if (value >= minX && value <= maxX) corridors.push({ axis: "x", value });
        }
      }
    }
    for (const segment of horizontal) {
      if (segment.maxX < minX || segment.minX > maxX) continue;
      for (const offset of offsets) {
        for (const value of [segment.y - offset, segment.y + offset]) {
          if (value >= minY && value <= maxY) corridors.push({ axis: "y", value });
        }
      }
    }
    const seen = new Set();
    return corridors.filter((corridor) => {
      const key = `${corridor.axis}:${corridor.value}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  };

  return { add, adjacentCorridors, crossingStats, hasStackedEndpoint, sharedSegmentStats };
}
