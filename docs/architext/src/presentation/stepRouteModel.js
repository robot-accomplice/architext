const routeClasses = {
  flow: "flow-step-route",
  sequence: "sequence-step-route"
};

export function stepRouteClassName(kind) {
  const className = routeClasses[kind];
  if (!className) throw new Error(`Unknown step route kind "${kind}"`);
  return className;
}

export function stepRouteMarkerClassName(className = "") {
  return `route-step-marker ${className}`.trim();
}

export function stepRouteLabelClassName(className = "") {
  return `route-step-label ${className}`.trim();
}
