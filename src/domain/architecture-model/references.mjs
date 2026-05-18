import { releaseSummaryFromDetail } from "./release-history.mjs";

export function validateArchitectureReferences(model) {
  const errors = [];
  const nodeIds = new Set(model.nodes.map((node) => node.id));
  const flowIds = new Set(model.flows.map((flow) => flow.id));
  const dataIds = new Set(model.dataClasses.map((item) => item.id));
  const decisionIds = new Set(model.decisions.map((item) => item.id));
  const riskIds = new Set(model.risks.map((item) => item.id));
  const viewIds = new Set(model.views.map((item) => item.id));

  const requireKnown = (id, known, context) => {
    if (!known.has(id)) errors.push(`${context} references unknown id "${id}"`);
  };

  requireKnown(model.manifest.defaultViewId, viewIds, "manifest.defaultViewId");

  for (const node of model.nodes) {
    for (const id of node.dependencies) requireKnown(id, nodeIds, `node ${node.id}.dependencies`);
    for (const id of node.dataHandled) requireKnown(id, dataIds, `node ${node.id}.dataHandled`);
    for (const id of node.relatedFlows) requireKnown(id, flowIds, `node ${node.id}.relatedFlows`);
    for (const id of node.relatedDecisions) requireKnown(id, decisionIds, `node ${node.id}.relatedDecisions`);
    for (const id of node.knownRisks) requireKnown(id, riskIds, `node ${node.id}.knownRisks`);
  }

  for (const flow of model.flows) {
    for (const id of flow.actors) requireKnown(id, nodeIds, `flow ${flow.id}.actors`);
    for (const step of flow.steps) {
      requireKnown(step.from, nodeIds, `flow ${flow.id} step ${step.id}.from`);
      requireKnown(step.to, nodeIds, `flow ${flow.id} step ${step.id}.to`);
      for (const id of step.data) requireKnown(id, dataIds, `flow ${flow.id} step ${step.id}.data`);
    }
  }

  for (const view of model.views) {
    for (const lane of view.lanes) {
      for (const id of lane.nodeIds) requireKnown(id, nodeIds, `view ${view.id} lane ${lane.id}`);
    }
  }

  if (model.releases) {
    validateReleaseReferences(model.releases, errors, { requireAllDetails: false });
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
    if (summary.status === "released" && !summary.releasedAt) {
      errors.push(`release index ${summary.id}.releasedAt is required for released entries`);
    }
    if ((summary.status === "active" || summary.status === "planning") && !summary.targetDate && !summary.targetWindow) {
      errors.push(`release index ${summary.id} requires targetDate or targetWindow`);
    }
  }

  for (const detail of releases.details) {
    const items = allReleaseItems(detail);
    const itemIds = new Set(items.map((item) => item.id));
    const workstreamIds = new Set(detail.workstreams.map((workstream) => workstream.id));

    if (detail.status === "released" && !detail.releasedAt) {
      errors.push(`release ${detail.id}.releasedAt is required for released entries`);
    }
    if ((detail.status === "active" || detail.status === "planning") && !detail.targetDate && !detail.targetWindow) {
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
      for (const itemId of blocker.itemIds) requireKnown(itemId, itemIds, `release ${detail.id} blocker ${blocker.id}.itemIds`);
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
