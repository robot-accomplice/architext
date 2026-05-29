import { planDiagram } from "./planDiagram.js";

self.onmessage = (event) => {
  const { key, input } = event.data;
  try {
    const plan = planDiagram(input);
    const { positionFor, ...cloneablePlan } = plan;
    self.postMessage({ key, plan: cloneablePlan });
  } catch (error) {
    self.postMessage({
      key,
      error: error instanceof Error ? error.message : String(error)
    });
  }
};
