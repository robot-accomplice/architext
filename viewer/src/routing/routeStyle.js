export function normalizeRouteStyle(style) {
  if (style === "spline" || style === "curved") return "spline";
  if (style === "straight") return "straight";
  return "orthogonal";
}
