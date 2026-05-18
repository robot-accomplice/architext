export const modeLabels = {
  flows: "Flows",
  sequence: "Sequence",
  c4: "C4",
  deployment: "Deployment",
  "data-risks": "Data/Risks",
  "release-truth": "Release Truth"
};

const modeViewTypes = {
  flows: ["system-map", "flow-explorer", "dataflow"],
  sequence: ["sequence"],
  c4: ["c4-context", "c4-container", "c4-component"],
  deployment: ["deployment"],
  "data-risks": ["risk-overlay", "dataflow"],
  "release-truth": []
};

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
  return Boolean(view && modeViewTypes[mode]?.includes(view.type));
}

export function defaultViewForMode(mode, views, fallback) {
  if (mode === "release-truth") return fallback;
  const types = modeViewTypes[mode] ?? [];
  return views.find((view) => types.includes(view.type)) ?? fallback;
}
