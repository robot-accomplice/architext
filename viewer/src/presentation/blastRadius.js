// Repository blast-radius model: search for a component/file/concept, then
// compute everything a chosen component reaches — its files, what it depends on,
// what depends on it (reverse edges), and the flows, decisions, risks, data, and
// views it participates in. Pure (no React/DOM) so it is unit-testable.

import { buildOwnerIndex, resolveOwner } from "./repoTreeModel.js";

const filePath = (entry) => (typeof entry === "string" ? entry : entry?.path);
const basename = (path) => {
  const slash = String(path).lastIndexOf("/");
  return slash === -1 ? String(path) : String(path).slice(slash + 1);
};

// Score a node against a lowercased query — best matching field wins so a name
// hit outranks a buried summary hit.
function scoreNode(node, q) {
  const name = String(node.name ?? node.id).toLowerCase();
  if (name === q) return 100;
  if (name.startsWith(q)) return 80;
  if (name.includes(q)) return 60;
  if (String(node.id).toLowerCase().includes(q)) return 50;
  if ((node.sourcePaths ?? []).some((p) => String(p).toLowerCase().includes(q))) return 40;
  if (String(node.summary ?? "").toLowerCase().includes(q)) return 30;
  if ((node.responsibilities ?? []).some((r) => String(r).toLowerCase().includes(q))) return 20;
  return 0;
}

function scorePath(path, q) {
  const base = basename(path).toLowerCase();
  if (base === q) return 100;
  if (base.startsWith(q)) return 80;
  if (base.includes(q)) return 60;
  if (String(path).toLowerCase().includes(q)) return 40;
  return 0;
}

// Search components (nodes) and files. Files resolve to their owning node so a
// file hit is a valid entry point into a component's blast radius.
export function searchRepository(model, files, query, limit = 12) {
  const q = String(query ?? "").trim().toLowerCase();
  if (!q) return { components: [], files: [] };

  const components = [];
  for (const node of model?.nodes ?? []) {
    const score = scoreNode(node, q);
    if (score > 0) components.push({ id: node.id, name: node.name ?? node.id, type: node.type, score });
  }
  components.sort((a, b) => b.score - a.score || a.name.localeCompare(b.name));

  const ownerIndex = buildOwnerIndex(model?.nodes ?? []);
  const fileMatches = [];
  for (const entry of files ?? []) {
    const path = filePath(entry);
    if (!path) continue;
    const score = scorePath(path, q);
    if (score === 0) continue;
    const owner = resolveOwner(path, ownerIndex);
    fileMatches.push({ path, ownerId: owner?.id ?? null, ownerName: owner?.name ?? owner?.id ?? null, score });
  }
  fileMatches.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));

  return { components: components.slice(0, limit), files: fileMatches.slice(0, limit) };
}

// Full reach of a single component. Returns null if the node is unknown.
export function blastRadiusForNode(model, files, nodeId) {
  const nodes = model?.nodes ?? [];
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const node = byId.get(nodeId);
  if (!node) return null;

  const nodeRef = (id) => {
    const n = byId.get(id);
    return n ? { id, name: n.name ?? id, type: n.type } : { id, name: id, type: "unknown" };
  };

  const ownerIndex = buildOwnerIndex(nodes);
  const ownedFiles = (files ?? [])
    .map((entry) => (typeof entry === "string" ? { path: entry, size: null, mtime: null } : entry))
    .filter((entry) => entry.path && resolveOwner(entry.path, ownerIndex)?.id === nodeId)
    .map((entry) => ({ path: entry.path, size: entry.size ?? null, mtime: entry.mtime ?? null }));

  // Forward edges (this -> X) and reverse edges (X -> this).
  const dependsOn = (node.dependencies ?? []).filter((id) => byId.has(id)).map(nodeRef);
  const dependents = nodes.filter((n) => (n.dependencies ?? []).includes(nodeId)).map((n) => nodeRef(n.id));

  const flows = model?.flows ?? [];
  const flowsById = new Map(flows.map((f) => [f.id, f]));
  const flowIds = new Set((node.relatedFlows ?? []).filter((id) => flowsById.has(id)));
  for (const flow of flows) {
    if ((flow.steps ?? []).some((step) => step.from === nodeId || step.to === nodeId)) flowIds.add(flow.id);
  }
  const relatedFlows = [...flowIds].map((id) => ({ id, name: flowsById.get(id)?.name ?? id }));

  const decisions = model?.decisions ?? [];
  const decById = new Map(decisions.map((d) => [d.id, d]));
  const decIds = new Set((node.relatedDecisions ?? []).filter((id) => decById.has(id)));
  for (const decision of decisions) {
    if ((decision.relatedNodes ?? []).includes(nodeId)) decIds.add(decision.id);
  }
  const relatedDecisions = [...decIds].map((id) => ({ id, title: decById.get(id)?.title ?? id }));

  const risks = model?.risks ?? [];
  const riskById = new Map(risks.map((r) => [r.id, r]));
  const riskIds = new Set((node.knownRisks ?? []).filter((id) => riskById.has(id)));
  for (const risk of risks) {
    if ((risk.relatedNodes ?? []).includes(nodeId)) riskIds.add(risk.id);
  }
  const relatedRisks = [...riskIds].map((id) => ({ id, title: riskById.get(id)?.title ?? id, severity: riskById.get(id)?.severity }));

  const dataClasses = model?.dataClasses ?? [];
  const dcById = new Map(dataClasses.map((d) => [d.id, d]));
  const dataHandled = (node.dataHandled ?? [])
    .filter((id) => dcById.has(id))
    .map((id) => ({ id, name: dcById.get(id).name, sensitivity: dcById.get(id).sensitivity }));

  const views = (model?.views ?? [])
    .filter((view) => (view.lanes ?? []).some((lane) => (lane.nodeIds ?? []).includes(nodeId)))
    .map((view) => ({ id: view.id, name: view.name, type: view.type }));

  return {
    node: nodeRef(nodeId),
    ownedFiles,
    declaredPaths: node.sourcePaths ?? [],
    dependsOn,
    dependents,
    flows: relatedFlows,
    decisions: relatedDecisions,
    risks: relatedRisks,
    dataHandled,
    views
  };
}
