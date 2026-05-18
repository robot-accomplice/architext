import { routeLength } from "./routeGeometry.js";

export function estimatedLabelBox(labelPoint, relationship) {
  if (!relationship) return null;
  if (relationship.relationshipType === "flow" || relationship.stepId) {
    return {
      x: labelPoint.x - 14,
      y: labelPoint.y - 12,
      width: 28,
      height: 24
    };
  }
  const text = relationship.label ?? relationship.id ?? "";
  const width = Math.max(24, Math.min(180, text.length * 6 + 12));
  return {
    x: labelPoint.x - width / 2,
    y: labelPoint.y - 9,
    width,
    height: 18
  };
}

export function withReadableLabel(route) {
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
