import { useEffect, useMemo, useState } from "react";
import { planDiagram } from "./planDiagram.js";

const ROUTING_LOADING_DELAY_MS = 1000;

/**
 * @typedef {{
 *   label: string;
 *   done: number;
 *   total: number;
 *   routesConsidered: number;
 * }} PlanningProgress
 */

/**
 * @typedef {{
 *   key: string;
 *   plan: any | null;
 *   planning: boolean;
 *   phase: string;
 *   progress: PlanningProgress | null;
 *   error: string | null;
 * }} PlannedDiagramState
 */

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

function attachPlanHelpers(plan) {
  return {
    ...plan,
    positionFor: (nodeId) => {
      const rect = plan.nodeRects.get(nodeId);
      return {
        x: rect?.x ?? 0,
        y: rect?.y ?? 0
      };
    }
  };
}

/**
 * @param {any} input
 * @returns {PlannedDiagramState}
 */
export function usePlannedDiagram(input) {
  const key = useMemo(() => planInputKey(input), [input]);
  /** @type {[PlannedDiagramState, import("react").Dispatch<import("react").SetStateAction<PlannedDiagramState>>]} */
  const [state, setState] = useState({
    key: "",
    plan: null,
    planning: false,
    phase: "",
    progress: null,
    error: null
  });

  useEffect(() => {
    let cancelled = false;
    let worker = null;

    setState((previous) => ({
      key,
      plan: previous.key === key ? previous.plan : null,
      planning: false,
      phase: "",
      progress: null,
      error: null
    }));

    const slowTimer = window.setTimeout(() => {
      if (cancelled) return;
      setState((previous) => previous.key === key ? { ...previous, planning: true } : previous);
    }, ROUTING_LOADING_DELAY_MS);

    const finishWithPlan = (plan) => {
      if (cancelled) return;
      window.clearTimeout(slowTimer);
      setState({
        key,
        plan: attachPlanHelpers(plan),
        planning: false,
        phase: "",
        progress: null,
        error: null
      });
    };

    const finishWithError = (message) => {
      if (cancelled) return;
      window.clearTimeout(slowTimer);
      setState({
        key,
        plan: null,
        planning: false,
        phase: "",
        progress: null,
        error: message
      });
    };

    if (typeof Worker === "undefined") {
      const timer = window.setTimeout(() => {
        try {
          const plan = planDiagram(input);
          const { positionFor, ...cloneablePlan } = plan;
          finishWithPlan(cloneablePlan);
        } catch (error) {
          finishWithError(error instanceof Error ? error.message : String(error));
        }
      }, 0);
      return () => {
        cancelled = true;
        window.clearTimeout(timer);
        window.clearTimeout(slowTimer);
      };
    }

    worker = new Worker(new URL("./planningWorker.js", import.meta.url), { type: "module" });
    worker.onmessage = (event) => {
      if (event.data.key !== key) return;
      if (event.data.phase) {
        // A pass started — show the overlay immediately (don't wait for the slow timer) so the
        // narration is visible even on fast diagrams; it just flashes by.
        window.clearTimeout(slowTimer);
        setState((previous) => previous.key === key ? { ...previous, planning: true, phase: event.data.phase } : previous);
        return;
      }
      if (event.data.progress) {
        // Live counters from inside the planner (edges done, routes considered) —
        // the honest "it is actually working" signal for long dense-flow plans.
        window.clearTimeout(slowTimer);
        setState((previous) => previous.key === key ? { ...previous, planning: true, progress: event.data.progress } : previous);
        return;
      }
      if (event.data.error) {
        finishWithError(event.data.error);
        return;
      }
      finishWithPlan(event.data.plan);
    };
    worker.onerror = (event) => {
      finishWithError(event.message || "Route planning failed.");
    };
    worker.postMessage({ key, input });

    return () => {
      cancelled = true;
      window.clearTimeout(slowTimer);
      worker?.terminate();
    };
  }, [key]);

  return state;
}

export function plannedCanvasFallback(input) {
  const maxRows = Math.max(...input.view.lanes.map((lane) => lane.nodeIds.filter((nodeId) => input.visibleNodeIds.has(nodeId)).length), 1);
  return {
    width: Math.max(input.minCanvasWidth, input.marginX * 2 + input.view.lanes.length * input.laneWidth + input.canvasExtraWidth),
    height: Math.max(input.minCanvasHeight, input.marginY + maxRows * input.rowGap + input.canvasExtraHeight)
  };
}
