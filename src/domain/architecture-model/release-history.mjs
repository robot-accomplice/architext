function releaseItems(detail) {
  return [
    ...detail.scope.required,
    ...detail.scope.planned,
    ...detail.scope.stretch,
    ...detail.scope.deferred,
    ...detail.scope.outOfScope
  ];
}

function countStatus(items, status) {
  return items.filter((item) => item.status === status).length;
}

export function deriveReleaseCounts(detail) {
  const items = releaseItems(detail);
  return {
    features: items.filter((item) => item.kind === "feature").length,
    bugFixes: items.filter((item) => item.kind === "bug-fix").length,
    workstreams: detail.workstreams.length,
    blockers: detail.blockers.length,
    complete: countStatus(items, "complete"),
    inProgress: countStatus(items, "in-progress"),
    planned: countStatus(items, "planned"),
    stretch: countStatus(items, "stretch")
  };
}

export function releaseSummaryFromDetail(detail, file) {
  return {
    id: detail.id,
    version: detail.version,
    name: detail.name,
    status: detail.status,
    posture: detail.posture,
    ...(detail.targetDate ? { targetDate: detail.targetDate } : {}),
    ...(detail.targetWindow ? { targetWindow: detail.targetWindow } : {}),
    ...(detail.releasedAt ? { releasedAt: detail.releasedAt } : {}),
    lastUpdated: detail.lastUpdated,
    summary: detail.summary,
    counts: deriveReleaseCounts(detail),
    file
  };
}

export function generatedReleaseIndex(existingIndex, detailEntries) {
  const summaries = detailEntries
    .map(({ detail, file }) => releaseSummaryFromDetail(detail, file))
    .sort((left, right) => releaseSortKey(left).localeCompare(releaseSortKey(right)));
  const summaryIds = new Set(summaries.map((summary) => summary.id));
  const currentReleaseId = summaryIds.has(existingIndex?.currentReleaseId)
    ? existingIndex.currentReleaseId
    : summaries.at(-1)?.id ?? "";
  return {
    currentReleaseId,
    releases: summaries
  };
}

export function releaseIndexGenerationChanges(existingIndex, generatedIndex) {
  const changes = [];
  if (!existingIndex) {
    changes.push("generate Release Truth history index from release detail files");
    return changes;
  }
  if (existingIndex.currentReleaseId !== generatedIndex.currentReleaseId) {
    changes.push("refresh Release Truth currentReleaseId from available detail files");
  }
  const existingById = new Map(existingIndex.releases.map((release) => [release.id, release]));
  const generatedById = new Map(generatedIndex.releases.map((release) => [release.id, release]));
  for (const release of generatedIndex.releases) {
    const existing = existingById.get(release.id);
    if (!existing) {
      changes.push(`add ${release.id} to Release Truth history`);
      continue;
    }
    if (!sameSummary(existing, release)) {
      changes.push(`refresh generated Release Truth history for ${release.id}`);
    }
  }
  for (const release of existingIndex.releases) {
    if (!generatedById.has(release.id)) {
      changes.push(`remove stale ${release.id} from Release Truth history`);
    }
  }
  return changes;
}

function releaseSortKey(release) {
  return release.releasedAt ?? release.targetDate ?? release.targetWindow ?? release.version ?? release.id;
}

function sameSummary(left, right) {
  return JSON.stringify(normalizeSummary(left)) === JSON.stringify(normalizeSummary(right));
}

function normalizeSummary(summary) {
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
