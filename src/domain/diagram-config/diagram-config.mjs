// User-configurable diagram layout parameters.
//
// Resolution precedence (lowest to highest): built-in defaults < user-global
// config (~/.architext/config.json) < project config (docs/architext/config.json).
//
// Validation is forgiving by design: a malformed or out-of-range value never
// breaks the viewer. Unknown keys are ignored, non-numeric values fall through
// to the lower-precedence layer, and out-of-range numbers are clamped — every
// such adjustment is reported as a warning so a misconfiguration is visible
// rather than silent.
//
// Scope (1.6.1 first cut): layout box/spacing, sequence-diagram spacing, fit-zoom
// bounds, and the single legibility-gap arrowhead fraction that the whole router
// derives mount spacing from. Cost-model weights are intentionally NOT exposed.

// Field specification: section -> field -> { default, min, max, step, unit, label }.
// The single source of truth for which parameters are configurable, their safe
// ranges, and how the config UI renders each control. Defaults mirror the
// hardcoded viewer values exactly so that an absent or empty config reproduces
// current rendering byte-for-byte. Section labels live in SECTION_LABELS.
export const DIAGRAM_CONFIG_FIELDS = {
  layout: {
    laneWidth: { default: 210, min: 60, max: 800, step: 2, unit: "px", label: "Column width" },
    rowGap: { default: 102, min: 20, max: 600, step: 2, unit: "px", label: "Row gap" },
    nodeWidth: { default: 136, min: 40, max: 600, step: 2, unit: "px", label: "Node width" },
    nodeHeight: { default: 54, min: 20, max: 400, step: 2, unit: "px", label: "Node height" },
    routeGutter: { default: 132, min: 20, max: 600, step: 2, unit: "px", label: "Route gutter" },
    marginY: { default: 104, min: 0, max: 600, step: 2, unit: "px", label: "Top margin" }
  },
  sequence: {
    participantWidth: { default: 146, min: 40, max: 800, step: 2, unit: "px", label: "Participant column width" },
    rowHeight: { default: 56, min: 16, max: 400, step: 2, unit: "px", label: "Message row height" },
    marginX: { default: 28, min: 0, max: 400, step: 2, unit: "px", label: "Side margin" }
  },
  zoom: {
    minFitZoom: { default: 0.15, min: 0.01, max: 1, step: 0.01, unit: "×", label: "Minimum fit zoom" },
    maxFitZoom: { default: 1.6, min: 0.5, max: 8, step: 0.1, unit: "×", label: "Maximum fit zoom" }
  },
  legibility: {
    // Fraction of a rendered arrowhead width (8px) used as the minimum gap at
    // which two parallel lines still read as two. 0.5 -> 4px, matching 1.6.0.
    gapArrowheads: { default: 0.5, min: 0, max: 4, step: 0.05, unit: "arrowheads", label: "Parallel-line gap" }
  }
};

export const SECTION_LABELS = {
  layout: "Layout & spacing",
  sequence: "Sequence diagram",
  zoom: "Fit zoom",
  legibility: "Line legibility"
};

export function defaultDiagramConfig() {
  const config = {};
  for (const [section, fields] of Object.entries(DIAGRAM_CONFIG_FIELDS)) {
    config[section] = {};
    for (const [field, spec] of Object.entries(fields)) {
      config[section][field] = spec.default;
    }
  }
  return config;
}

// Extract only the known, valid, in-range numeric overrides from one raw layer
// (e.g. a parsed config file). Returns { overrides, warnings }. `overrides`
// contains just the fields this layer actually sets; absent/invalid fields are
// omitted so the next-lower layer shows through.
export function normalizeDiagramConfigLayer(raw, { source = "config" } = {}) {
  const overrides = {};
  const warnings = [];
  if (raw == null) return { overrides, warnings };
  if (typeof raw !== "object" || Array.isArray(raw)) {
    warnings.push(`${source}: expected a JSON object, ignoring entire config.`);
    return { overrides, warnings };
  }

  for (const [section, value] of Object.entries(raw)) {
    const fields = DIAGRAM_CONFIG_FIELDS[section];
    if (!fields) {
      warnings.push(`${source}: unknown section "${section}" ignored.`);
      continue;
    }
    if (value == null || typeof value !== "object" || Array.isArray(value)) {
      warnings.push(`${source}: section "${section}" must be an object, ignored.`);
      continue;
    }
    for (const [field, rawNumber] of Object.entries(value)) {
      const spec = fields[field];
      if (!spec) {
        warnings.push(`${source}: unknown field "${section}.${field}" ignored.`);
        continue;
      }
      if (typeof rawNumber !== "number" || !Number.isFinite(rawNumber)) {
        warnings.push(`${source}: "${section}.${field}" must be a finite number, ignored.`);
        continue;
      }
      const clamped = Math.min(spec.max, Math.max(spec.min, rawNumber));
      if (clamped !== rawNumber) {
        warnings.push(`${source}: "${section}.${field}" ${rawNumber} clamped to ${clamped} (allowed ${spec.min}–${spec.max}).`);
      }
      (overrides[section] ??= {})[field] = clamped;
    }
  }
  return { overrides, warnings };
}

// Resolve the effective config from ordered raw layers (lowest precedence
// first). Each entry is { raw, source }. Returns { config, warnings }.
export function resolveDiagramConfig(layers = []) {
  const config = defaultDiagramConfig();
  const warnings = [];
  for (const { raw, source } of layers) {
    const normalized = normalizeDiagramConfigLayer(raw, { source });
    warnings.push(...normalized.warnings);
    for (const [section, fields] of Object.entries(normalized.overrides)) {
      Object.assign(config[section], fields);
    }
  }
  // Cross-field invariant: a usable fit-zoom window requires min <= max. If a
  // configuration inverts them, fall back to defaults for the zoom section so
  // the viewer never receives an empty zoom range.
  if (config.zoom.minFitZoom > config.zoom.maxFitZoom) {
    warnings.push(`zoom: minFitZoom ${config.zoom.minFitZoom} exceeds maxFitZoom ${config.zoom.maxFitZoom}; reverting zoom to defaults.`);
    config.zoom = { ...defaultDiagramConfig().zoom };
  }
  return { config, warnings };
}

// Reduce a (full or partial) config to only the fields that differ from the
// built-in defaults, dropping empty sections. Used when persisting from the UI
// so a saved config.json carries just the user's real overrides — keeping the
// precedence chain meaningful (an unset field still falls through to a lower
// layer) and the file minimal. Returns {} when nothing differs.
export function diffDiagramConfigFromDefaults(config) {
  const overrides = {};
  if (config == null || typeof config !== "object") return overrides;
  for (const [section, fields] of Object.entries(DIAGRAM_CONFIG_FIELDS)) {
    const provided = config[section];
    if (provided == null || typeof provided !== "object") continue;
    for (const [field, spec] of Object.entries(fields)) {
      const value = provided[field];
      if (typeof value === "number" && Number.isFinite(value) && value !== spec.default) {
        (overrides[section] ??= {})[field] = value;
      }
    }
  }
  return overrides;
}
