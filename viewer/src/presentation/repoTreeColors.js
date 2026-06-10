// Shared owner-color model for the Repo Tree view. The tree workspace and the
// left-nav legend both import from here so a node's color and its legend swatch
// can never drift apart.

// C4 lens: owning node type -> the existing diagram color palette.
export const C4_COLOR = {
  actor: "var(--pink)",
  "software-system": "var(--cyan)",
  client: "var(--blue)",
  service: "var(--purple)",
  worker: "var(--purple)",
  queue: "var(--orange)",
  "data-store": "var(--green)",
  "external-service": "var(--muted)",
  module: "var(--c4-module)",
  "deployment-unit": "var(--c4-deployment)"
};

export const C4_TYPE_LABEL = {
  actor: "Actor",
  "software-system": "Software system",
  client: "Client",
  service: "Service",
  worker: "Worker",
  queue: "Queue",
  "data-store": "Data store",
  "external-service": "External service",
  module: "Module",
  "deployment-unit": "Deployment unit"
};

// Flow lens: stable per-flow palette assigned by flow order.
export const FLOW_PALETTE = [
  "var(--blue)", "var(--green)", "var(--orange)", "var(--pink)",
  "var(--purple)", "var(--cyan)", "var(--yellow)", "var(--red)"
];

export function buildFlowColorMap(flows) {
  const map = new Map();
  (flows ?? []).forEach((flow, index) => map.set(flow.id, FLOW_PALETTE[index % FLOW_PALETTE.length]));
  return map;
}

const DIM = "var(--dim)";

// Color for an owning node under the active lens. Returns null when the lens
// has nothing to say (flow lens, node with no related flow).
export function colorForOwner(owner, lens, flowColorMap) {
  if (!owner) return null;
  if (lens === "c4") return C4_COLOR[owner.type] ?? DIM;
  const flowId = owner.relatedFlows?.[0];
  return flowId ? flowColorMap.get(flowId) ?? DIM : null;
}

// The legend entries to show for the active lens: only the owner types (or
// flows) that actually own at least one path, so the key matches what's on
// screen. Each entry: { key, label, color }.
export function ownerLegend(nodes, flows, lens) {
  const owners = (nodes ?? []).filter((node) => (node.sourcePaths ?? []).length > 0);
  if (lens === "c4") {
    const seen = new Map();
    for (const node of owners) {
      if (!seen.has(node.type)) {
        seen.set(node.type, { key: node.type, label: C4_TYPE_LABEL[node.type] ?? node.type, color: C4_COLOR[node.type] ?? DIM });
      }
    }
    return Array.from(seen.values());
  }
  const flowColorMap = buildFlowColorMap(flows);
  const flowsById = new Map((flows ?? []).map((flow) => [flow.id, flow]));
  const seen = new Map();
  for (const node of owners) {
    const flowId = node.relatedFlows?.[0];
    if (!flowId || seen.has(flowId)) continue;
    const flow = flowsById.get(flowId);
    seen.set(flowId, { key: flowId, label: flow?.name ?? flowId, color: flowColorMap.get(flowId) ?? DIM });
  }
  return Array.from(seen.values());
}
