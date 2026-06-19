const BASE_LAYOUT = {
  nodeWidth: 136,
  nodeHeight: 54,
  laneWidth: 210,
  rowGap: 102,
  routeGutter: 132,
  marginY: 104,
  minCanvasWidth: 0,
  minCanvasHeight: 340,
  canvasExtraHeight: 88
};

const DENSE_VIEW_TYPES = new Set(["dataflow", "deployment", "flow-explorer", "risk-overlay"]);

/**
 * @param {*} view
 * @param {number} [relationshipCount]
 * @param {{nodeWidth?:number,nodeHeight?:number,laneWidth?:number,rowGap?:number,routeGutter?:number,marginY?:number}|null} [layoutConfig]
 */
export function diagramLayoutFor(view, relationshipCount = 0, layoutConfig = null) {
  const maxRows = Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1);
  const isDenseTopology = DENSE_VIEW_TYPES.has(view.type) && (maxRows >= 5 || relationshipCount >= 8);
  // User overrides win over the auto-selected (default vs dense) value, and are
  // applied before deriving marginX/canvasExtraWidth so an overridden routeGutter
  // cascades. With layoutConfig null, every `??` falls through to the original
  // value, so default rendering is byte-identical.
  const o = layoutConfig ?? {};
  const nodeWidth = o.nodeWidth ?? BASE_LAYOUT.nodeWidth;
  const nodeHeight = o.nodeHeight ?? BASE_LAYOUT.nodeHeight;
  const laneWidth = o.laneWidth ?? (isDenseTopology ? 240 : BASE_LAYOUT.laneWidth);
  const rowGap = o.rowGap ?? (isDenseTopology ? 176 : BASE_LAYOUT.rowGap);
  const routeGutter = o.routeGutter ?? (isDenseTopology ? 180 : BASE_LAYOUT.routeGutter);
  const marginY = o.marginY ?? BASE_LAYOUT.marginY;
  return {
    ...BASE_LAYOUT,
    nodeWidth,
    nodeHeight,
    laneWidth,
    rowGap,
    routeGutter,
    marginY,
    marginX: routeGutter + 48,
    minCanvasHeight: isDenseTopology ? 560 : BASE_LAYOUT.minCanvasHeight,
    canvasExtraWidth: routeGutter
  };
}
