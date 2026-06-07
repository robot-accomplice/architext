const c4Layouts = {
  "c4-context": {
    nodeWidth: 184,
    nodeHeight: 96,
    laneWidth: 280,
    rowGap: 144,
    marginX: 104,
    marginY: 104,
    minCanvasWidth: 920,
    minCanvasHeight: 500,
    canvasExtraWidth: 112,
    canvasExtraHeight: 120,
    boundaryLabel: "System boundary"
  },
  "c4-container": {
    nodeWidth: 176,
    nodeHeight: 104,
    laneWidth: 270,
    rowGap: 156,
    marginX: 104,
    marginY: 108,
    minCanvasWidth: 960,
    minCanvasHeight: 540,
    canvasExtraWidth: 128,
    canvasExtraHeight: 128,
    boundaryLabel: "Container boundary"
  },
  "c4-component": {
    nodeWidth: 168,
    nodeHeight: 98,
    laneWidth: 252,
    rowGap: 140,
    marginX: 96,
    marginY: 104,
    minCanvasWidth: 900,
    minCanvasHeight: 520,
    canvasExtraWidth: 112,
    canvasExtraHeight: 120,
    boundaryLabel: "Component scope"
  }
};

export function c4LayoutFor(viewType) {
  return c4Layouts[viewType] ?? c4Layouts["c4-container"];
}
