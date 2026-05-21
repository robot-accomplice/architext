export const modeLabels = {
  flows: "Flows",
  sequence: "Sequence",
  c4: "C4",
  deployment: "Deployment",
  "data-risks": "Data/Risks",
  "release-truth": "Release Truth",
  rules: "Rules"
};

const modeHashAliases = {
  flows: "flows",
  flow: "flows",
  sequence: "sequence",
  c4: "c4",
  deployment: "deployment",
  datarisks: "data-risks",
  "data-risks": "data-risks",
  releasetruth: "release-truth",
  "release-truth": "release-truth",
  rules: "rules"
};

const modeViewTypes = {
  flows: ["system-map", "flow-explorer", "workflow", "dataflow"],
  sequence: ["sequence"],
  c4: ["c4-context", "c4-container", "c4-component", "c4-code"],
  deployment: ["deployment"],
  "data-risks": ["risk-overlay", "dataflow"],
  "release-truth": [],
  rules: []
};

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
