// Shared plan-request builder: the ONE place that turns (view, flow, layout
// config, routing style) into the diagram planner's input. Both the viewer
// (main.tsx) and `architext serve`'s plan-precompute farm import these, so the
// planInputKey computed in the browser and the key the server precomputed under
// are derived from identical construction — strict parity by shared code, not
// by convention. A precomputed plan is only ever served on an exact key match;
// any drift surfaces as a cache miss (browser plans locally), never as a wrong
// diagram. This module is also the contract surface a future Go backend must
// reproduce byte-for-byte.

import { diagramLayoutFor } from "./diagramLayout.js";
import { decisionBranchTargets, flowStepDisplayIndexes } from "./flowStepDisplayModel.js";
import { nodeLanePosition, preferredDecisionBranchSide, preferredDecisionBranchEndSide } from "./decisionBranchModel.js";

export function decisionNodeId(stepId) {
  return `decision:${stepId}`;
}

export function decisionTip(rect, side) {
  const center = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  const radius = rect.width / Math.SQRT2;
  if (side === "left") return { x: center.x - radius, y: center.y };
  if (side === "right") return { x: center.x + radius, y: center.y };
  if (side === "top") return { x: center.x, y: center.y - radius };
  return { x: center.x, y: center.y + radius };
}

export function decisionRouteRect(rect) {
  return {
    ...rect,
    fixedPorts: true,
    sideAnchors: {
      left: decisionTip(rect, "left"),
      right: decisionTip(rect, "right"),
      top: decisionTip(rect, "top"),
      bottom: decisionTip(rect, "bottom")
    }
  };
}

// Connector from the affiliated node down to the diamond's node-facing (top) tip.
// The diamond sits below its node, so the TOP point is the node side; branches
// only ever use the other three tips (left/right/bottom), so they never collide
// with this connection.
export function decisionConnectorRoute(decisionNode, componentRect) {
  const x = componentRect.x + componentRect.width / 2;
  const decisionTop = decisionTip(decisionNode.rect, "top");
  return {
    points: [
      { x, y: componentRect.y + componentRect.height },
      decisionTop
    ]
  };
}

export function buildFlowRelationships(flow, view) {
  if (!flow) return [];
  const displayIndexes = flowStepDisplayIndexes(flow.steps);
  const decisionStepByTarget = new Map(flow.steps.filter((step) => step.kind === "decision").map((step) => [step.to, step]));
  return flow.steps.map((step, index) => {
    const displayIndex = displayIndexes.get(step.id) ?? index + 1;
    const decisionStep = step.outcome ? decisionStepByTarget.get(step.from) : null;
    const decisionPosition = decisionStep ? nodeLanePosition(view, step.from) : null;
    const branchStartSide = decisionStep && decisionPosition ? preferredDecisionBranchSide(view, decisionPosition, step.to) : undefined;
    return {
      id: step.id,
      from: decisionStep ? decisionNodeId(decisionStep.id) : step.from,
      to: step.to,
      label: `${displayIndex}. ${step.action}`,
      summary: step.summary,
      relationshipType: "flow",
      stepId: decisionStep ? decisionStep.id : step.id,
      branchStepId: decisionStep ? step.id : undefined,
      flowId: flow.id,
      displayIndex,
      kind: step.kind,
      returnOf: step.returnOf,
      stepKind: step.kind,
      outcome: step.outcome,
      componentFrom: step.from,
      componentTo: step.to,
      preferredStartSide: branchStartSide,
      preferredEndSide: branchStartSide && decisionPosition ? preferredDecisionBranchEndSide(view, decisionPosition, step.to, branchStartSide) : undefined
    };
  });
}

export function buildDecisionNodes(flow, view, layout) {
  if (!flow) return [];
  const { nodeWidth, nodeHeight, laneWidth, rowGap, marginX, marginY } = layout;
  const branchedTargets = decisionBranchTargets(flow.steps);
  const displayIndexes = flowStepDisplayIndexes(flow.steps);
  return flow.steps
    .filter((step) => step.kind === "decision" && branchedTargets.has(step.to))
    .flatMap((step) => {
      const position = nodeLanePosition(view, step.to);
      if (!position) return [];
      const { laneIndex, rowIndex } = position;
      return [{
        id: decisionNodeId(step.id),
        action: step.action,
        componentId: step.to,
        displayIndex: displayIndexes.get(step.id) ?? 0,
        rect: {
          x: marginX + laneIndex * laneWidth + nodeWidth / 2 - 19,
          y: marginY + rowIndex * rowGap + nodeHeight + 22,
          width: 38,
          height: 38
        },
        laneIndex,
        rowIndex
      }];
    });
}

// The exact planner-input shape usePlannedDiagram keys on. Shared so the
// viewer's live input and the server's precomputed input cannot drift.
export function assemblePlanInput({ view, relationships, visibleNodeIds, layout, decisionNodes, style }) {
  return {
    view,
    relationships,
    visibleNodeIds,
    nodeWidth: layout.nodeWidth,
    nodeHeight: layout.nodeHeight,
    laneWidth: layout.laneWidth,
    rowGap: layout.rowGap,
    marginX: layout.marginX,
    marginY: layout.marginY,
    minCanvasWidth: layout.minCanvasWidth,
    minCanvasHeight: layout.minCanvasHeight,
    canvasExtraWidth: layout.canvasExtraWidth,
    canvasExtraHeight: layout.canvasExtraHeight,
    extraNodeRects: new Map(decisionNodes.map((node) => [node.id, decisionRouteRect(node.rect)])),
    extraLaneIndexByNode: new Map(decisionNodes.map((node) => [node.id, node.laneIndex])),
    extraRowIndexByNode: new Map(decisionNodes.map((node) => [node.id, node.rowIndex])),
    style
  };
}

// One-call flow-mode plan request: what the precompute farm enumerates and what
// the viewer assembles for flows-mode diagrams.
export function buildFlowPlanRequest({ view, flow, layoutConfig, style }) {
  const relationships = buildFlowRelationships(flow, view);
  const layout = diagramLayoutFor(view, relationships.length, layoutConfig);
  const decisionNodes = buildDecisionNodes(flow, view, layout);
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  return {
    relationships,
    layout,
    decisionNodes,
    planInput: assemblePlanInput({ view, relationships, visibleNodeIds, layout, decisionNodes, style })
  };
}
