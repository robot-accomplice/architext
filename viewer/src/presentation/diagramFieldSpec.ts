import type { DiagramFieldsSpec, DiagramSectionLabels } from "./DiagramConfigPanel.js";

// Bundled copy of the diagram config field spec so the settings panel ALWAYS
// renders its controls, independent of any /api/config fetch. The server still
// provides resolved values + persistence, and echoes its own spec on GET, but
// the UI never depends on that response to show controls (a failed or stale
// fetch must not hide the panel).
//
// KEEP IN SYNC with src/domain/diagram-config/diagram-config.mjs
// (DIAGRAM_CONFIG_FIELDS + SECTION_LABELS). The two live in separate packages
// (viewer vs CLI) and cannot share a module; defaults/ranges must match.
export const DIAGRAM_FIELD_SPEC: DiagramFieldsSpec = {
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
    gapArrowheads: { default: 0.5, min: 0, max: 4, step: 0.05, unit: "arrowheads", label: "Parallel-line gap" }
  }
};

export const DIAGRAM_SECTION_LABELS: DiagramSectionLabels = {
  layout: "Layout & spacing",
  sequence: "Sequence diagram",
  zoom: "Fit zoom",
  legibility: "Line legibility"
};
