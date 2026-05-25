import { releaseSummaryFromDetail } from "./release-history.mjs";

function slug(value) {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .replace(/-{2,}/g, "-");
}

function releaseIdForVersion(version) {
  return `v${version.replaceAll(".", "-")}`;
}

function releaseFileForId(id) {
  return `${id}.json`;
}

function allReleaseItems(releaseDetail) {
  return [
    ...releaseDetail.scope.required,
    ...releaseDetail.scope.planned,
    ...releaseDetail.scope.stretch,
    ...releaseDetail.scope.deferred,
    ...releaseDetail.scope.outOfScope
  ];
}

function releaseScopeEntries(scope) {
  return [
    ["required", scope.required],
    ["planned", scope.planned],
    ["stretch", scope.stretch],
    ["deferred", scope.deferred],
    ["outOfScope", scope.outOfScope]
  ];
}

export function nextMinorVersion(releaseIndex) {
  const versions = releaseIndex.releases
    .map((release) => release.version)
    .map((version) => version.match(/^(\d+)\.(\d+)\.(\d+)$/))
    .filter(Boolean)
    .map((match) => match.slice(1).map(Number))
    .sort((left, right) => left[0] - right[0] || left[1] - right[1] || left[2] - right[2]);
  const latest = versions.at(-1) ?? [0, 0, 0];
  return `${latest[0]}.${latest[1] + 1}.0`;
}

function uniqueId(base, usedIds) {
  const normalized = slug(base) || "release-item";
  if (!usedIds.has(normalized)) {
    usedIds.add(normalized);
    return normalized;
  }
  let index = 2;
  while (usedIds.has(`${normalized}-${index}`)) index += 1;
  const id = `${normalized}-${index}`;
  usedIds.add(id);
  return id;
}

function workstreamIdForSection(section, usedIds) {
  return uniqueId(section, usedIds);
}

const releaseScopes = new Set(["required", "planned", "stretch", "deferred", "outOfScope"]);

function normalizedScope(value) {
  if (value === "out-of-scope") return "outOfScope";
  return releaseScopes.has(value) ? value : "planned";
}

function releaseStatusForScope(scope) {
  if (scope === "deferred") return "deferred";
  if (scope === "outOfScope") return "cut";
  return "planned";
}

function releaseItemFromRoadmap(item, workstreamId, dateAdded, scope = "planned") {
  return {
    id: item.id,
    title: item.title,
    kind: item.kind,
    status: releaseStatusForScope(scope),
    summary: item.summary,
    ...(item.priority ? { priority: item.priority } : {}),
    source: "roadmap",
    dateAdded,
    workstreamId,
    dependsOn: item.dependsOn ? [...item.dependsOn] : [],
    evidence: item.evidence ? [...item.evidence] : []
  };
}

function releaseItemFromAdHoc(item, usedItemIds, workstreamId, dateAdded) {
  const scope = normalizedScope(item.scope);
  if (!item.kind) {
    throw new Error(`Ad hoc release item "${item.title}" must include a kind.`);
  }
  if (!item.priority) {
    throw new Error(`Ad hoc release item "${item.title}" must include a priority.`);
  }
  return {
    id: item.id ?? uniqueId(item.title, usedItemIds),
    title: item.title,
    kind: item.kind,
    status: releaseStatusForScope(scope),
    summary: item.summary?.trim() || item.title,
    priority: item.priority,
    source: "ad-hoc",
    dateAdded,
    workstreamId,
    dependsOn: item.dependsOn ? [...item.dependsOn] : [],
    evidence: item.evidence ? [...item.evidence] : []
  };
}

function mergedReleaseItem(existing, proposed) {
  if (!existing) return proposed;
  const preserveImplementationStatus = ["complete", "in-progress", "blocked"].includes(existing.status)
    && !["deferred", "cut"].includes(proposed.status);
  return {
    ...proposed,
    source: existing.source ?? proposed.source,
    dateAdded: existing.dateAdded ?? proposed.dateAdded,
    status: preserveImplementationStatus ? existing.status : proposed.status,
    ...(existing.owner ? { owner: existing.owner } : {}),
    ...(existing.rationale ? { rationale: existing.rationale } : {}),
    ...(existing.decisionSource ? { decisionSource: existing.decisionSource } : {}),
    dependsOn: existing.dependsOn?.length ? existing.dependsOn : proposed.dependsOn,
    evidence: existing.evidence?.length ? existing.evidence : proposed.evidence,
    ...(existing.deferredToReleaseId ? { deferredToReleaseId: existing.deferredToReleaseId } : {}),
    ...(existing.deferredToVersion ? { deferredToVersion: existing.deferredToVersion } : {})
  };
}

function mergedWorkstream(existing, proposed) {
  if (!existing) return proposed;
  return {
    ...proposed,
    status: existing.status,
    posture: existing.posture,
    summary: existing.summary || proposed.summary,
    progress: existing.progress ?? proposed.progress,
    evidence: existing.evidence?.length ? existing.evidence : proposed.evidence
  };
}

export function mergeExistingReleasePlan(existingDetail, proposedDetail) {
  if (!existingDetail || existingDetail.id !== proposedDetail.id) return proposedDetail;
  const existingItemsById = new Map(allReleaseItems(existingDetail).map((item) => [item.id, item]));
  const existingWorkstreamsById = new Map(existingDetail.workstreams.map((workstream) => [workstream.id, workstream]));
  const scope = Object.fromEntries(releaseScopeEntries(proposedDetail.scope).map(([key, items]) => [
    key,
    items.map((item) => mergedReleaseItem(existingItemsById.get(item.id), item))
  ]));
  return {
    ...proposedDetail,
    status: existingDetail.status === "draft" ? proposedDetail.status : existingDetail.status,
    posture: existingDetail.posture,
    summary: existingDetail.summary || proposedDetail.summary,
    targetDate: existingDetail.targetDate ?? proposedDetail.targetDate,
    targetWindow: existingDetail.targetWindow ?? proposedDetail.targetWindow,
    releasedAt: existingDetail.releasedAt ?? proposedDetail.releasedAt,
    updateSource: proposedDetail.updateSource,
    scope,
    workstreams: proposedDetail.workstreams.map((workstream) => mergedWorkstream(existingWorkstreamsById.get(workstream.id), workstream)),
    blockers: existingDetail.blockers,
    dependencies: existingDetail.dependencies,
    evidence: existingDetail.evidence?.length ? existingDetail.evidence : proposedDetail.evidence
  };
}

function releaseItemSection(item, releaseDetail) {
  const workstream = releaseDetail.workstreams.find((candidate) => candidate.id === item.workstreamId);
  return workstream?.name ?? "Ad hoc";
}

function roadmapStatusFromReleaseItem(item) {
  if (["complete", "in-progress", "deferred", "cut"].includes(item.status)) return item.status;
  return "planned";
}

function roadmapItemFromReleaseItem(item, releaseDetail) {
  return {
    id: item.id,
    title: item.title,
    summary: item.summary,
    kind: item.kind,
    status: roadmapStatusFromReleaseItem(item),
    ...(item.priority ? { priority: item.priority } : {}),
    section: releaseItemSection(item, releaseDetail),
    targetReleaseId: releaseDetail.id,
    ...(item.dateAdded ? { dateAdded: item.dateAdded } : {}),
    ...(item.evidence?.length ? { evidence: item.evidence } : {})
  };
}

function roadmapChangeForItem(item, existing, releaseDetail) {
  if (!existing) {
    return {
      action: "add",
      id: item.id,
      title: item.title,
      targetReleaseId: releaseDetail.id,
      source: item.source ?? "ad-hoc"
    };
  }
  if (existing.targetReleaseId === releaseDetail.id) {
    return {
      action: "unchanged",
      id: item.id,
      title: item.title,
      targetReleaseId: releaseDetail.id,
      source: item.source ?? "roadmap"
    };
  }
  return {
    action: "retarget",
    id: item.id,
    title: item.title,
    fromReleaseId: existing.targetReleaseId ?? "",
    targetReleaseId: releaseDetail.id,
    source: item.source ?? "roadmap"
  };
}

export function buildReleasePlan({
  releaseIndex,
  roadmapItems,
  selectedRoadmapItemIds,
  itemScopes = {},
  adHocItems = [],
  projectName,
  version = nextMinorVersion(releaseIndex),
  theme,
  now
}) {
  if (!now) throw new Error("buildReleasePlan requires an explicit now timestamp.");
  const selectedIds = new Set(selectedRoadmapItemIds);
  const roadmapIds = new Set(roadmapItems.map((item) => item.id));
  for (const selectedId of selectedIds) {
    if (!roadmapIds.has(selectedId)) {
      throw new Error(`selectedRoadmapItemIds references unknown id "${selectedId}"`);
    }
  }
  const id = releaseIdForVersion(version);
  const usedItemIds = new Set(roadmapItems.map((item) => item.id));
  const workstreamIds = new Set();
  const workstreamsBySection = new Map();
  const scope = {
    required: [],
    planned: [],
    stretch: [],
    deferred: [],
    outOfScope: []
  };

  const workstreamForSection = (section) => {
    const name = section || "Ad hoc";
    if (workstreamsBySection.has(name)) return workstreamsBySection.get(name);
    const id = workstreamIdForSection(name, workstreamIds);
    const workstream = {
      id,
      name,
      owner: "maintainer",
      status: "planned",
      posture: "on-track",
      summary: `${name} release scope.`,
      progress: 0,
      itemIds: [],
      evidence: []
    };
    workstreamsBySection.set(name, workstream);
    return workstream;
  };

  for (const item of roadmapItems) {
    if (!selectedIds.has(item.id)) continue;
    if (item.targetReleaseId && item.targetReleaseId !== id && item.status !== "deferred") {
      throw new Error(`Roadmap item "${item.title}" is already committed to ${item.targetReleaseId}. Defer it before moving it to ${id}.`);
    }
    const workstream = workstreamForSection(item.section);
    const releaseScope = normalizedScope(itemScopes[item.id]);
    const releaseItem = releaseItemFromRoadmap(item, workstream.id, now, releaseScope);
    scope[releaseScope].push(releaseItem);
    workstream.itemIds.push(releaseItem.id);
  }

  for (const item of adHocItems) {
    const workstream = workstreamForSection(item.section ?? "Ad hoc");
    const releaseItem = releaseItemFromAdHoc(item, usedItemIds, workstream.id, now);
    scope[normalizedScope(item.scope)].push(releaseItem);
    workstream.itemIds.push(releaseItem.id);
  }

  const scopedItems = [
    ...scope.required,
    ...scope.planned,
    ...scope.stretch,
    ...scope.deferred,
    ...scope.outOfScope
  ];
  const title = theme ? `${projectName} ${version} ${theme}` : `${projectName} ${version}`;
  return {
    id,
    version,
    name: title,
    status: "planned",
    posture: "on-track",
    summary: theme
      ? `${theme} release plan.`
      : `${projectName} ${version} release plan.`,
    targetWindow: "Next release",
    lastUpdated: now,
    updateSource: "Release Planning",
    scope,
    workstreams: [...workstreamsBySection.values()],
    blockers: [],
    milestones: [{
      id: "planned-scope",
      label: "Planned scope selected",
      status: "planned",
      targetWindow: "Release planning",
      order: 1,
      itemIds: scopedItems.map((item) => item.id)
    }],
    dependencies: [],
    evidence: []
  };
}

export function releasePlanChanges({
  releaseIndex,
  roadmap,
  releaseDetail,
  file = releaseFileForId(releaseDetail.id),
  mode = "approve"
}) {
  const existingRelease = releaseIndex.releases.find((release) => release.id === releaseDetail.id);
  const roadmapItemsById = new Map(roadmap.items.map((item) => [item.id, item]));
  const roadmapChanges = mode === "draft"
    ? []
    : allReleaseItems(releaseDetail)
      .filter((item) => item.status !== "cut")
      .map((item) => roadmapChangeForItem(item, roadmapItemsById.get(item.id), releaseDetail));

  return {
    releaseFile: {
      action: existingRelease ? "replace" : "create",
      file,
      id: releaseDetail.id,
      name: releaseDetail.name
    },
    releaseIndex: {
      action: existingRelease ? "replace-summary" : "add-summary",
      currentReleaseId: mode === "draft" ? releaseIndex.currentReleaseId : releaseDetail.id,
      releaseCount: existingRelease ? releaseIndex.releases.length : releaseIndex.releases.length + 1
    },
    roadmap: {
      add: roadmapChanges.filter((change) => change.action === "add").length,
      retarget: roadmapChanges.filter((change) => change.action === "retarget").length,
      unchanged: roadmapChanges.filter((change) => change.action === "unchanged").length,
      changes: roadmapChanges
    }
  };
}

export function saveReleasePlanDraft({
  releaseIndex,
  roadmap,
  releaseDetail,
  file = releaseFileForId(releaseDetail.id)
}) {
  const draftDetail = {
    ...releaseDetail,
    status: "draft"
  };
  const releaseSummary = releaseSummaryFromDetail(draftDetail, file);
  const releases = releaseIndex.releases.filter((release) => release.id !== draftDetail.id);
  releases.push(releaseSummary);

  return {
    releaseIndex: {
      ...releaseIndex,
      releases
    },
    roadmap,
    releaseFile: {
      file,
      detail: draftDetail
    },
    changes: releasePlanChanges({ releaseIndex, roadmap, releaseDetail: draftDetail, file, mode: "draft" })
  };
}

export function approveReleasePlan({
  releaseIndex,
  roadmap,
  releaseDetail,
  file = releaseFileForId(releaseDetail.id)
}) {
  const approvedDetail = {
    ...releaseDetail,
    status: releaseDetail.status === "draft" ? "planned" : releaseDetail.status
  };
  const releaseSummary = releaseSummaryFromDetail(approvedDetail, file);
  const releases = releaseIndex.releases.filter((release) => release.id !== approvedDetail.id);
  releases.push(releaseSummary);

  const roadmapItemsById = new Map(roadmap.items.map((item) => [item.id, { ...item }]));
  for (const item of allReleaseItems(approvedDetail)) {
    if (item.status === "cut") continue;
    const existing = roadmapItemsById.get(item.id);
    if (existing) {
      roadmapItemsById.set(item.id, {
        ...existing,
        targetReleaseId: approvedDetail.id,
        ...(item.dateAdded && !existing.dateAdded ? { dateAdded: item.dateAdded } : {})
      });
      continue;
    }
    roadmapItemsById.set(item.id, roadmapItemFromReleaseItem(item, approvedDetail));
  }

  return {
    releaseIndex: {
      ...releaseIndex,
      currentReleaseId: approvedDetail.id,
      releases
    },
    roadmap: {
      ...roadmap,
      items: [...roadmapItemsById.values()]
    },
    releaseFile: {
      file,
      detail: approvedDetail
    },
    changes: releasePlanChanges({ releaseIndex, roadmap, releaseDetail: approvedDetail, file })
  };
}
