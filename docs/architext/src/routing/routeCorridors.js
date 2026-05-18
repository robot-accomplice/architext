import { uniqueRounded } from "./routeGeometry.js";
import { PORT_STUB } from "./routePorts.js";

export const CORRIDOR_PADDING = 10;

function interiorCorridors(fromRect, toRect) {
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
}

function mergeCorridors(corridors) {
  const seen = new Set();
  return corridors.filter((corridor) => {
    const key = `${corridor.axis}:${corridor.value}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function freeSpaceCorridors(visibleRects, canvasWidth, canvasHeight) {
  const minX = 24;
  const maxX = canvasWidth - 24;
  const minY = 30;
  const maxY = canvasHeight - 24;
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
}

export function edgeCorridors(fromRect, toRect, diagramCorridors) {
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
}
