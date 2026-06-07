import { uniqueRounded } from "./routeGeometry.js";
import { PORT_STUB } from "./routePorts.js";
import { CANVAS_INSET, dedupeBy } from "./routeConstants.js";

export const CORRIDOR_PADDING = 10;
const GUTTER_LANE_SPACING = 42;
const MAX_GUTTER_LANES = 6;

function gutterLaneValues(start, end, min, max) {
  const width = end - start;
  if (width <= CORRIDOR_PADDING * 3) return [];
  const laneCount = Math.min(MAX_GUTTER_LANES, Math.max(1, Math.floor(width / GUTTER_LANE_SPACING)));
  return Array.from({ length: laneCount }, (_, index) => Math.round(start + (width * (index + 1)) / (laneCount + 1)))
    .filter((value) => value > min && value < max);
}

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
  return dedupeBy(corridors, (corridor) => `${corridor.axis}:${corridor.value}`);
}

export function freeSpaceCorridors(visibleRects, canvasWidth, canvasHeight) {
  const minX = CANVAS_INSET.left;
  const maxX = canvasWidth - CANVAS_INSET.right;
  const minY = CANVAS_INSET.top;
  const maxY = canvasHeight - CANVAS_INSET.bottom;
  const verticalEdges = uniqueRounded([minX, maxX, ...visibleRects.flatMap((rect) => [rect.x, rect.x + rect.width])]).sort((a, b) => a - b);
  const horizontalEdges = uniqueRounded([minY, maxY, ...visibleRects.flatMap((rect) => [rect.y, rect.y + rect.height])]).sort((a, b) => a - b);
  const corridors = [];

  for (let index = 0; index < verticalEdges.length - 1; index += 1) {
    const left = verticalEdges[index];
    const right = verticalEdges[index + 1];
    for (const value of gutterLaneValues(left, right, minX, maxX)) corridors.push({ axis: "x", value });
  }
  for (let index = 0; index < horizontalEdges.length - 1; index += 1) {
    const top = horizontalEdges[index];
    const bottom = horizontalEdges[index + 1];
    for (const value of gutterLaneValues(top, bottom, minY, maxY)) corridors.push({ axis: "y", value });
  }
  return corridors;
}

export function edgeCorridors(fromRect, toRect, diagramCorridors, options = {}) {
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
  const exterior = (axis, min, max) => {
    const axisCorridors = diagramCorridors
      .filter((corridor) => corridor.axis === axis)
      .sort((left, right) => left.value - right.value);
    const before = axisCorridors.filter((corridor) => corridor.value < min).at(-1);
    const after = axisCorridors.find((corridor) => corridor.value > max);
    return [before, after].filter(Boolean);
  };
  const horizontalOverlap = fromRect.x < toRect.x + toRect.width && fromRect.x + fromRect.width > toRect.x;
  const verticalOverlap = fromRect.y < toRect.y + toRect.height && fromRect.y + fromRect.height > toRect.y;
  const exteriorCorridors = options.includeExterior
    ? [
        ...(horizontalOverlap ? exterior("x", minX, maxX) : []),
        ...(verticalOverlap ? exterior("y", minY, maxY) : [])
      ]
    : [];
  return mergeCorridors([
    ...interiorCorridors(fromRect, toRect),
    ...exteriorCorridors,
    ...closest("x"),
    ...closest("y")
  ]);
}
