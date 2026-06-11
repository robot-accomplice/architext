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
 *   timing: { totalMs: number; phases: Array<{ label: string; ms: number }>; routesConsidered: number } | null;
 *   error: string | null;
 * }} PlannedDiagramState
 */

export const PLAN_TIMING_STORAGE_KEY = "architext.planTimings";
const PLAN_TIMING_HISTORY_LIMIT = 20;

// Assemble the per-phase breakdown for a completed plan from the phase marks
// collected while it ran. Pure so it is unit-testable: marks are
// [{ label, at }] in ms relative to the same clock as startedAt/completedAt.
export function buildPlanTiming({ startedAt, completedAt, marks, lastProgress }) {
  const totalMs = Math.max(0, Math.round(completedAt - startedAt));
  const phases = marks.map((mark, index) => {
    const end = index + 1 < marks.length ? marks[index + 1].at : completedAt;
    return { label: mark.label, ms: Math.max(0, Math.round(end - mark.at)) };
  });
  return {
    totalMs,
    phases,
    routesConsidered: lastProgress?.routesConsidered ?? 0
  };
}

// Persist slow-plan timings so the breakdown is reportable after the overlay is
// gone: a structured console record plus a localStorage ring buffer readable via
// JSON.parse(localStorage.getItem(PLAN_TIMING_STORAGE_KEY)). Only plans slow
// enough to have shown the loading overlay are recorded.
function persistPlanTiming(timing) {
  try {
    console.info(
      `[architext] routed in ${(timing.totalMs / 1000).toFixed(1)}s`,
      Object.fromEntries(timing.phases.map((phase) => [phase.label, `${(phase.ms / 1000).toFixed(1)}s`])),
      `${timing.routesConsidered.toLocaleString()} routes considered`
    );
    const history = JSON.parse(window.localStorage.getItem(PLAN_TIMING_STORAGE_KEY) ?? "[]");
    history.push({ v: 1, at: new Date().toISOString(), ...timing });
    window.localStorage.setItem(PLAN_TIMING_STORAGE_KEY, JSON.stringify(history.slice(-PLAN_TIMING_HISTORY_LIMIT)));
  } catch {
    // Telemetry must never break planning (storage full/disabled, SSR, etc.).
  }
}

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
    timing: null,
    error: null
  });

  useEffect(() => {
    let cancelled = false;
    let worker = null;

    const startedAt = performance.now();
    const phaseMarks = [];
    let lastProgress = null;

    setState((previous) => ({
      key,
      plan: previous.key === key ? previous.plan : null,
      planning: false,
      phase: "",
      progress: null,
      timing: null,
      error: null
    }));

    const slowTimer = window.setTimeout(() => {
      if (cancelled) return;
      setState((previous) => previous.key === key ? { ...previous, planning: true } : previous);
    }, ROUTING_LOADING_DELAY_MS);

    const finishWithPlan = (plan) => {
      if (cancelled) return;
      window.clearTimeout(slowTimer);
      const timing = buildPlanTiming({ startedAt, completedAt: performance.now(), marks: phaseMarks, lastProgress });
      // Persist only plans slow enough to have shown the loading overlay, so the
      // record matches what the user experienced and fast re-plans don't spam.
      if (timing.totalMs >= ROUTING_LOADING_DELAY_MS) persistPlanTiming(timing);
      setState({
        key,
        plan: attachPlanHelpers(plan),
        planning: false,
        phase: "",
        progress: null,
        timing,
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
        timing: null,
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
        phaseMarks.push({ label: event.data.phase, at: performance.now() });
        window.clearTimeout(slowTimer);
        setState((previous) => previous.key === key ? { ...previous, planning: true, phase: event.data.phase } : previous);
        return;
      }
      if (event.data.progress) {
        // Live counters from inside the planner (edges done, routes considered) —
        // the honest "it is actually working" signal for long dense-flow plans.
        lastProgress = event.data.progress;
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
