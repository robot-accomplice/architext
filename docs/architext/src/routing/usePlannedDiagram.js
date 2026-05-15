import { useEffect, useState } from "react";
import { planDiagram } from "./planDiagram.js";

const ROUTING_LOADING_DELAY_MS = 1000;

/**
 * @typedef {{
 *   key: string;
 *   plan: any | null;
 *   planning: boolean;
 *   error: string | null;
 * }} PlannedDiagramState
 */

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
      flowId: relationship.flowId
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
  const key = planInputKey(input);
  /** @type {[PlannedDiagramState, import("react").Dispatch<import("react").SetStateAction<PlannedDiagramState>>]} */
  const [state, setState] = useState({
    key: "",
    plan: null,
    planning: false,
    error: null
  });

  useEffect(() => {
    let cancelled = false;
    let worker = null;

    setState((previous) => ({
      key,
      plan: previous.key === key ? previous.plan : null,
      planning: false,
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
