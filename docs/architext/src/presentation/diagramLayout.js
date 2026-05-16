const BASE_LAYOUT = {
  nodeWidth: 136,
  nodeHeight: 54,
  laneWidth: 210,
  rowGap: 102,
  routeGutter: 132,
  marginY: 76,
  minCanvasWidth: 0,
  minCanvasHeight: 340,
  canvasExtraHeight: 88
};

const DENSE_VIEW_TYPES = new Set(["dataflow", "deployment", "risk-overlay"]);

export function diagramLayoutFor(view, relationshipCount = 0) {
  const maxRows = Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1);
  const isDenseTopology = DENSE_VIEW_TYPES.has(view.type) && (maxRows >= 5 || relationshipCount >= 8);
  const routeGutter = isDenseTopology ? 180 : BASE_LAYOUT.routeGutter;
  return {
    ...BASE_LAYOUT,
    laneWidth: isDenseTopology ? 240 : BASE_LAYOUT.laneWidth,
    rowGap: isDenseTopology ? 176 : BASE_LAYOUT.rowGap,
    routeGutter,
    marginX: routeGutter + 48,
    minCanvasHeight: isDenseTopology ? 560 : BASE_LAYOUT.minCanvasHeight,
    canvasExtraWidth: routeGutter
  };
}
