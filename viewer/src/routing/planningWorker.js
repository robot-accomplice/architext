import { planDiagram } from "./planDiagram.js";

self.onmessage = (event) => {
  const { key, input } = event.data;
  try {
    const plan = planDiagram({
      ...input,
      onPhase: (phase) => self.postMessage({ key, phase }),
      // Throttled at the source (routeEdges emits at most ~8/s), so the worker
      // forwards every report without further rate-limiting.
      onProgress: (progress) => self.postMessage({ key, progress })
    });
    const { positionFor, ...cloneablePlan } = plan;
    self.postMessage({ key, plan: cloneablePlan });
  } catch (error) {
    self.postMessage({
      key,
      error: error instanceof Error ? error.message : String(error)
    });
  }
};
