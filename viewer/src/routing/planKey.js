// Canonical plan-input key: the EXACT serialized planner input that both the
// viewer's fetch-first lookup and `architext serve`'s precompute farm hash
// under (sha256). Lives in its own React-free module because the serve-side
// farm — and therefore every packed CLI invocation — imports it from Node,
// where the viewer's React dependency does not exist at runtime.

function sortedMapEntries(map, projectValue) {
  if (!map) return [];
  return Array.from(map.entries())
    .map(([nodeId, value]) => [nodeId, projectValue(value)])
    .sort(([left], [right]) => String(left).localeCompare(String(right)));
}

function roundRect(rect) {
  return [Math.round(rect.x), Math.round(rect.y), Math.round(rect.width), Math.round(rect.height)];
}

export function planInputKey(input) {
  return JSON.stringify({
    view: {
      id: input.view.id,
      type: input.view.type,
      lanes: input.view.lanes.map((lane) => [lane.id, lane.nodeIds])
    },
    relationships: input.relationships.map((relationship) => ({
      id: relationship.id,
      from: relationship.from,
      to: relationship.to,
      label: relationship.label,
      relationshipType: relationship.relationshipType,
      stepId: relationship.stepId,
      flowId: relationship.flowId,
      kind: relationship.kind,
      returnOf: relationship.returnOf,
      outcome: relationship.outcome,
      displayIndex: relationship.displayIndex,
      preferredStartSide: relationship.preferredStartSide,
      preferredEndSide: relationship.preferredEndSide
    })),
    visibleNodeIds: Array.from(input.visibleNodeIds).sort(),
    nodeWidth: input.nodeWidth,
    nodeHeight: input.nodeHeight,
    laneWidth: input.laneWidth,
    rowGap: input.rowGap,
    marginX: input.marginX,
    marginY: input.marginY,
    minCanvasWidth: input.minCanvasWidth,
    minCanvasHeight: input.minCanvasHeight,
    canvasExtraWidth: input.canvasExtraWidth,
    canvasExtraHeight: input.canvasExtraHeight,
    extraNodeRects: sortedMapEntries(input.extraNodeRects, roundRect),
    extraLaneIndexByNode: sortedMapEntries(input.extraLaneIndexByNode, (value) => value),
    extraRowIndexByNode: sortedMapEntries(input.extraRowIndexByNode, (value) => value),
    scoreEdgeProximity: Boolean(input.scoreEdgeProximity),
    style: input.style
  });
}
