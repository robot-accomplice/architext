export function childC4ViewForNode(views, activeView, nodeId) {
  if (!activeView?.type?.startsWith("c4-")) return null;
  const nextType = childC4Type(activeView.type);
  if (!nextType) return null;
  return views.find((view) => view.type === nextType && view.scopeNodeId === nodeId) ?? null;
}

export function c4DrilldownUnavailableReason(activeView, node) {
  if (!activeView?.type?.startsWith("c4-") || !node) return "";
  const nextType = childC4Type(activeView.type);
  if (!nextType) {
    return `${node.name} is already at the deepest supported C4 level for this diagram.`;
  }
  if (node.type === "actor") {
    return `${node.name} is an actor. Actors sit outside the system decomposition, so there is no ${c4TypeLabel(nextType)} drilldown.`;
  }
  if (node.type === "external-service") {
    return `${node.name} is an external dependency and is outside the purview of this C4 decomposition.`;
  }
  return `No ${c4TypeLabel(nextType)} diagram has been defined for ${node.name}.`;
}

function childC4Type(type) {
  if (type === "c4-context") return "c4-container";
  if (type === "c4-container") return "c4-component";
  if (type === "c4-component") return "c4-code";
  return null;
}

function c4TypeLabel(type) {
  if (type === "c4-container") return "container";
  if (type === "c4-component") return "component";
  if (type === "c4-code") return "code";
  return "child";
}
