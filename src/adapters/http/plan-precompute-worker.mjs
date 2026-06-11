// Worker-thread runner for the serve-side plan precompute farm: receives a
// planner input (worker_threads structured clone carries its Maps/Sets
// faithfully), runs the exact same planDiagram the viewer's web worker runs,
// and posts the plan back. One job at a time — the farm serializes the queue.
import { parentPort } from "node:worker_threads";
import { planDiagram } from "../../../viewer/src/routing/planDiagram.js";

parentPort.on("message", ({ jobId, planInput }) => {
  try {
    const { positionFor, ...plan } = planDiagram(planInput);
    parentPort.postMessage({ jobId, plan });
  } catch (error) {
    parentPort.postMessage({ jobId, error: error instanceof Error ? error.message : String(error) });
  }
});
