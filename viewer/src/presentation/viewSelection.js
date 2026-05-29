const MODE_DEFINITIONS = [
  { id: "flows", label: "Flows", aliases: ["flows", "flow"], viewTypes: ["system-map", "flow-explorer", "workflow", "dataflow"] },
  { id: "sequence", label: "Sequence", aliases: ["sequence"], viewTypes: ["sequence"] },
  { id: "c4", label: "C4", aliases: ["c4"], viewTypes: ["c4-context", "c4-container", "c4-component", "c4-code"] },
  { id: "deployment", label: "Deployment", aliases: ["deployment"], viewTypes: ["deployment"] },
  { id: "data-risks", label: "Data/Risks", aliases: ["datarisks", "data-risks"], viewTypes: ["risk-overlay", "dataflow"] },
  { id: "release-truth", label: "Release Truth", aliases: ["releasetruth", "release-truth"], viewTypes: [] },
  { id: "rules", label: "Rules", aliases: ["rules"], viewTypes: [] }
];

export const modeLabels = Object.fromEntries(MODE_DEFINITIONS.map((mode) => [mode.id, mode.label]));
const modeHashAliases = Object.fromEntries(MODE_DEFINITIONS.flatMap((mode) => mode.aliases.map((alias) => [alias, mode.id])));
const modeViewTypes = Object.fromEntries(MODE_DEFINITIONS.map((mode) => [mode.id, mode.viewTypes]));

export function modeForHash(hash) {
  const key = String(hash ?? "")
    .replace(/^#/, "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "");
  return modeHashAliases[key] ?? null;
}

export function hashForMode(mode) {
  return `#${mode.replace(/-/g, "")}`;
}

export function modeForView(view) {
  if (!view) return "flows";
  if (view.type === "sequence") return "sequence";
  if (view.type?.startsWith("c4-")) return "c4";
  if (view.type === "deployment") return "deployment";
  if (view.type === "risk-overlay") return "data-risks";
  return "flows";
}

export function viewBelongsToMode(view, mode) {
  if (mode === "release-truth") return true;
  if (mode === "rules") return true;
  return Boolean(view && modeViewTypes[mode]?.includes(view.type));
}

export function viewTypesForMode(mode) {
  return modeViewTypes[mode] ?? [];
}

export function defaultViewForMode(mode, views, fallback) {
  if (mode === "release-truth") return fallback;
  if (mode === "rules") return fallback;
  const types = viewTypesForMode(mode);
  return views.find((view) => types.includes(view.type)) ?? fallback;
}

export function flowEndpointIds(flow) {
  return new Set(flow?.steps?.flatMap((step) => [step.from, step.to]) ?? []);
}

export function viewNodeIds(view) {
  return new Set(view?.lanes?.flatMap((lane) => lane.nodeIds ?? []) ?? []);
}

export function flowCompatibleWithView(flow, view) {
  const endpointIds = flowEndpointIds(flow);
  if (endpointIds.size === 0) return Boolean(flow && view);
  const nodeIds = viewNodeIds(view);
  return Array.from(endpointIds).every((id) => nodeIds.has(id));
}

export function compatibleFlowViewsForFlow(views, flow) {
  const flowProjectionTypes = new Set(viewTypesForMode("flows"));
  return views.filter((view) => flowProjectionTypes.has(view.type) && flowCompatibleWithView(flow, view));
}

export function compatibleFlowsForView(flows, view) {
  return flows.filter((flow) => flowCompatibleWithView(flow, view));
}

export function defaultViewForFlow(mode, currentView, views, flow, fallback) {
  if (mode !== "flows") return currentView ?? defaultViewForMode(mode, views, fallback);
  const compatibleViews = compatibleFlowViewsForFlow(views, flow);
  const authoredProjection = compatibleViews.find((view) => view.type !== "system-map");
  if (currentView?.type === "system-map" && authoredProjection) return authoredProjection;
  if (flowCompatibleWithView(flow, currentView)) return currentView;
  return authoredProjection ?? compatibleViews[0] ?? defaultViewForMode(mode, views, fallback);
}

export function defaultFlowForView(view, currentFlow, flows, fallback) {
  if (flowCompatibleWithView(currentFlow, view)) return currentFlow;
  return compatibleFlowsForView(flows, view)[0] ?? fallback;
}
