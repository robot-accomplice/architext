import { releaseSummaryFromDetail } from "./release-history.mjs";

export function validateArchitectureReferences(model) {
  const errors = [];
  const arrayField = (owner, field, required = false) => {
    const value = owner?.[field];
    if (Array.isArray(value)) return value;
    if (value === undefined && !required) return [];
    errors.push(`${field} must be an array`);
    return [];
  };
  const nestedArray = (owner, field, context) => {
    const value = owner?.[field];
    if (Array.isArray(value)) return value;
    if (value === undefined) return [];
    errors.push(`${context}.${field} must be an array`);
    return [];
  };
  const nodes = arrayField(model, "nodes", true);
  const flows = arrayField(model, "flows", true);
  const dataClasses = arrayField(model, "dataClasses", true);
  const decisions = arrayField(model, "decisions", true);
  const risks = arrayField(model, "risks", true);
  const views = arrayField(model, "views", true);
  const nodeIds = new Set(nodes.map((node) => node.id));
  const flowIds = new Set(flows.map((flow) => flow.id));
  const dataIds = new Set(dataClasses.map((item) => item.id));
  const decisionIds = new Set(decisions.map((item) => item.id));
  const riskIds = new Set(risks.map((item) => item.id));
  const viewIds = new Set(views.map((item) => item.id));

  const requireKnown = (id, known, context) => {
    if (!known.has(id)) errors.push(`${context} references unknown id "${id}"`);
  };

  requireKnown(model.manifest.defaultViewId, viewIds, "manifest.defaultViewId");

  for (const node of nodes) {
    for (const id of nestedArray(node, "dependencies", `node ${node.id}`)) requireKnown(id, nodeIds, `node ${node.id}.dependencies`);
    for (const id of nestedArray(node, "dataHandled", `node ${node.id}`)) requireKnown(id, dataIds, `node ${node.id}.dataHandled`);
    for (const id of nestedArray(node, "relatedFlows", `node ${node.id}`)) requireKnown(id, flowIds, `node ${node.id}.relatedFlows`);
    for (const id of nestedArray(node, "relatedDecisions", `node ${node.id}`)) requireKnown(id, decisionIds, `node ${node.id}.relatedDecisions`);
    for (const id of nestedArray(node, "knownRisks", `node ${node.id}`)) requireKnown(id, riskIds, `node ${node.id}.knownRisks`);
  }

  for (const flow of flows) {
    for (const id of nestedArray(flow, "actors", `flow ${flow.id}`)) requireKnown(id, nodeIds, `flow ${flow.id}.actors`);
    for (const step of nestedArray(flow, "steps", `flow ${flow.id}`)) {
      if (!step.from) errors.push(`flow ${flow.id} step ${step.id}.from is required`);
      else requireKnown(step.from, nodeIds, `flow ${flow.id} step ${step.id}.from`);
      if (!step.to) errors.push(`flow ${flow.id} step ${step.id}.to is required`);
      else requireKnown(step.to, nodeIds, `flow ${flow.id} step ${step.id}.to`);
      for (const id of nestedArray(step, "data", `flow ${flow.id} step ${step.id}`)) requireKnown(id, dataIds, `flow ${flow.id} step ${step.id}.data`);
    }
  }

  for (const view of views) {
    if (view.scopeNodeId) requireKnown(view.scopeNodeId, nodeIds, `view ${view.id}.scopeNodeId`);
    for (const lane of nestedArray(view, "lanes", `view ${view.id}`)) {
      for (const id of nestedArray(lane, "nodeIds", `view ${view.id} lane ${lane.id}`)) requireKnown(id, nodeIds, `view ${view.id} lane ${lane.id}`);
    }
  }

  if (model.releases) {
    validateReleaseReferences(model.releases, errors, { requireAllDetails: false });
  }
  if (model.roadmap && model.releases) {
    const releaseIds = new Set(model.releases.index.releases.map((release) => release.id));
    for (const item of model.roadmap) {
      if (item.targetReleaseId) requireKnown(item.targetReleaseId, releaseIds, `roadmap item ${item.id}.targetReleaseId`);
    }
  }

  return errors;
}

function allReleaseItems(detail) {
  return [
    ...detail.scope.required,
    ...detail.scope.planned,
    ...detail.scope.stretch,
    ...detail.scope.deferred,
    ...detail.scope.outOfScope
  ];
}

function releaseItemCanBeBlocked(status) {
  return !["complete", "deferred", "cut"].includes(status);
}

export function validateReleaseReferences(releases, errors = [], options = {}) {
  const requireAllDetails = options.requireAllDetails ?? true;
  const releaseIds = new Set(releases.index.releases.map((release) => release.id));
  const detailIds = new Set(releases.details.map((detail) => detail.id));

  const requireKnown = (id, known, context) => {
    if (!known.has(id)) errors.push(`${context} references unknown id "${id}"`);
  };

  requireKnown(releases.index.currentReleaseId, releaseIds, "releases.currentReleaseId");

  for (const summary of releases.index.releases) {
    if (requireAllDetails) requireKnown(summary.id, detailIds, `release index ${summary.id}.file`);
    const detail = releases.details.find((item) => item.id === summary.id);
    if (!detail) continue;
    if (detail.version !== summary.version) {
      errors.push(`release ${summary.id}.version does not match release index`);
    }
    if (detail.status !== summary.status) {
      errors.push(`release ${summary.id}.status does not match release index`);
    }
    if (!sameGeneratedReleaseSummary(summary, detail)) {
      errors.push(`release ${summary.id}.index summary is stale; regenerate Release Truth history`);
    }
    if (summary.status === "completed" && !summary.releasedAt) {
      errors.push(`release index ${summary.id}.releasedAt is required for completed entries`);
    }
    if ((summary.status === "implementing" || summary.status === "planned" || summary.status === "draft") && !summary.targetDate && !summary.targetWindow) {
      errors.push(`release index ${summary.id} requires targetDate or targetWindow`);
    }
  }

  for (const detail of releases.details) {
    const items = allReleaseItems(detail);
    const itemIds = new Set(items.map((item) => item.id));
    const itemsById = new Map(items.map((item) => [item.id, item]));
    const workstreamIds = new Set(detail.workstreams.map((workstream) => workstream.id));

    if (detail.status === "completed" && !detail.releasedAt) {
      errors.push(`release ${detail.id}.releasedAt is required for completed entries`);
    }
    if ((detail.status === "implementing" || detail.status === "planned" || detail.status === "draft") && !detail.targetDate && !detail.targetWindow) {
      errors.push(`release ${detail.id} requires targetDate or targetWindow`);
    }

    for (const item of items) {
      if (item.workstreamId) requireKnown(item.workstreamId, workstreamIds, `release ${detail.id} item ${item.id}.workstreamId`);
      for (const dependencyId of item.dependsOn ?? []) requireKnown(dependencyId, itemIds, `release ${detail.id} item ${item.id}.dependsOn`);
    }

    for (const workstream of detail.workstreams) {
      for (const itemId of workstream.itemIds) requireKnown(itemId, itemIds, `release ${detail.id} workstream ${workstream.id}.itemIds`);
    }

    for (const blocker of detail.blockers) {
      for (const itemId of blocker.itemIds) {
        requireKnown(itemId, itemIds, `release ${detail.id} blocker ${blocker.id}.itemIds`);
        const item = itemsById.get(itemId);
        if (item && !releaseItemCanBeBlocked(item.status)) {
          errors.push(`release ${detail.id} blocker ${blocker.id}.itemIds references ${item.status} item "${itemId}"`);
        }
      }
    }

    for (const milestone of detail.milestones) {
      for (const itemId of milestone.itemIds) requireKnown(itemId, itemIds, `release ${detail.id} milestone ${milestone.id}.itemIds`);
    }

    for (const dependency of detail.dependencies) {
      requireKnown(dependency.from, itemIds, `release ${detail.id} dependency ${dependency.id}.from`);
      requireKnown(dependency.to, itemIds, `release ${detail.id} dependency ${dependency.id}.to`);
    }
  }

  return errors;
}

function sameGeneratedReleaseSummary(summary, detail) {
  const generated = releaseSummaryFromDetail(detail, summary.file);
  return JSON.stringify(normalizeReleaseSummary(summary)) === JSON.stringify(normalizeReleaseSummary(generated));
}

function normalizeReleaseSummary(summary) {
  return {
    id: summary.id,
    version: summary.version,
    name: summary.name,
    status: summary.status,
    posture: summary.posture,
    targetDate: summary.targetDate ?? null,
    targetWindow: summary.targetWindow ?? null,
    releasedAt: summary.releasedAt ?? null,
    lastUpdated: summary.lastUpdated,
    summary: summary.summary,
    counts: summary.counts,
    file: summary.file
  };
}
