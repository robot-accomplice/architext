const orderedFlowModes = new Set(["flows", "sequence", "data-risks"]);

export function modeShowsOrderedFlow(mode) {
  return orderedFlowModes.has(mode);
}

export function modeUsesStructuralRelationships(mode) {
  return mode === "c4" || mode === "deployment";
}
