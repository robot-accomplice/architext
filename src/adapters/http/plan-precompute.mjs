// Serve-side plan precompute farm.
//
// The browser pays an order-of-magnitude penalty over Node for dense-flow route
// planning (measured 144x on the separation pass), so `architext serve`
// precomputes every flow x compatible-flow-view plan in a worker thread at
// startup and again whenever the data (or diagram config) changes. Plans are
// stored under sha256(planInputKey) — the canonical hash of the EXACT planner
// input, built by the same shared planRequest module the viewer uses — and the
// viewer renders a precomputed plan only on an exact key match. A miss (config
// draft in play, data just changed, anything at all) falls back to in-browser
// planning, so a stale or mismatched plan is structurally impossible to serve:
// the cache can miss, it cannot lie.
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { Worker } from "node:worker_threads";
import { fileURLToPath } from "node:url";
import { buildFlowPlanRequest } from "../../../viewer/src/presentation/planRequest.js";
import { planInputKey } from "../../../viewer/src/routing/usePlannedDiagram.js";
import { serializePlan } from "../../../viewer/src/routing/planCodec.js";
import { viewTypesForMode, flowCompatibleWithView } from "../../../viewer/src/presentation/viewSelection.js";
import { diagramConfigGetPayload } from "./diagram-config-api.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const defaultWorkerPath = path.join(here, "plan-precompute-worker.mjs");

export function planKeyHash(key) {
  return createHash("sha256").update(key).digest("hex");
}

// Every flow x flows-mode view the viewer could ask the planner for, with the
// exact planner input the viewer would build (shared construction).
export async function enumerateFlowPlanRequests({ dataDir, layoutConfig, readFileFn = readFile }) {
  const [flows, views] = await Promise.all([
    readFileFn(path.join(dataDir, "flows.json"), "utf8").then((raw) => JSON.parse(raw).flows ?? []),
    readFileFn(path.join(dataDir, "views.json"), "utf8").then((raw) => JSON.parse(raw).views ?? [])
  ]);
  const flowViewTypes = new Set(viewTypesForMode("flows"));
  const requests = [];
  for (const view of views) {
    if (!flowViewTypes.has(view.type)) continue;
    for (const flow of flows) {
      if (!flowCompatibleWithView(flow, view)) continue;
      const { planInput } = buildFlowPlanRequest({ view, flow, layoutConfig, style: "orthogonal" });
      const key = planInputKey(planInput);
      requests.push({ key, hash: planKeyHash(key), planInput, flowId: flow.id, viewId: view.id });
    }
  }
  return requests;
}

export function createPlanPrecomputeFarm({
  target,
  dataDirFn,
  // MUST be the same config source the viewer consumes (/api/config payload,
  // defaults filled) — enumerating from the raw saved file diverges the layout
  // numbers and therefore every key (found live: laneWidth undefined vs 210).
  loadConfigFn = diagramConfigGetPayload,
  workerPath = defaultWorkerPath,
  log = (message) => console.log(message)
}) {
  const store = new Map();
  let generation = 0;
  let queue = [];
  let worker = null;
  let busy = false;
  let disposed = false;
  let jobSeq = 0;
  let inFlight = null;

  const ensureWorker = () => {
    if (worker || disposed) return;
    worker = new Worker(workerPath);
    worker.unref();
    worker.on("message", ({ jobId, plan, error }) => {
      if (inFlight && inFlight.jobId === jobId) {
        const job = inFlight;
        inFlight = null;
        busy = false;
        if (!error && job.generation === generation) {
          store.set(job.hash, JSON.stringify(serializePlan(restoreCollections(plan))));
        } else if (error) {
          log(`[architext] plan precompute failed for flow ${job.flowId} on ${job.viewId}: ${error}`);
        }
        pump();
      }
    });
    worker.on("error", (error) => {
      log(`[architext] plan precompute worker error: ${error.message}`);
      worker = null;
      busy = false;
      inFlight = null;
      if (queue.length > 0) pump();
    });
  };

  // worker_threads structured clone preserves Map/Set, so the plan arrives with
  // its collections intact — this is a type-level identity, kept as a function
  // so a future transport change fails here loudly.
  const restoreCollections = (plan) => plan;

  const pump = () => {
    if (busy || disposed || queue.length === 0) return;
    const job = queue.shift();
    if (job.generation !== generation) {
      pump();
      return;
    }
    ensureWorker();
    if (!worker) return;
    busy = true;
    inFlight = job;
    worker.postMessage({ jobId: job.jobId, planInput: job.planInput });
  };

  const refresh = async () => {
    if (disposed) return;
    generation += 1;
    const thisGeneration = generation;
    store.clear();
    queue = [];
    try {
      const config = await loadConfigFn(target);
      const requests = await enumerateFlowPlanRequests({
        dataDir: dataDirFn(target),
        layoutConfig: config?.diagram?.layout
      });
      if (thisGeneration !== generation || disposed) return;
      queue = requests.map((request) => ({ ...request, generation: thisGeneration, jobId: (jobSeq += 1) }));
      log(`[architext] precomputing ${queue.length} diagram plan(s) in the background`);
      pump();
    } catch (error) {
      log(`[architext] plan precompute enumeration failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  return {
    refresh,
    lookup: (hash) => store.get(hash),
    stats: () => ({ plans: store.size, pending: queue.length + (busy ? 1 : 0), generation }),
    dispose: () => {
      disposed = true;
      queue = [];
      worker?.terminate();
      worker = null;
    }
  };
}
