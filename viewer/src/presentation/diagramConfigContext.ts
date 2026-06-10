import { createContext, useContext } from "react";

// User-configurable diagram parameters, resolved server-side by `architext serve`
// (GET /api/config) from defaults < ~/.architext/config.json < project config.
// All fields optional: an absent config (or a viewer served without the API,
// e.g. a static build) falls back to the hardcoded defaults at each call site.
export interface DiagramConfig {
  layout?: {
    nodeWidth?: number;
    nodeHeight?: number;
    laneWidth?: number;
    rowGap?: number;
    routeGutter?: number;
    marginY?: number;
  };
  sequence?: {
    participantWidth?: number;
    rowHeight?: number;
    marginX?: number;
  };
  zoom?: {
    minFitZoom?: number;
    maxFitZoom?: number;
  };
  legibility?: {
    gapArrowheads?: number;
  };
}

export const DiagramConfigContext = createContext<DiagramConfig | null>(null);

export function useDiagramConfig(): DiagramConfig | null {
  return useContext(DiagramConfigContext);
}
