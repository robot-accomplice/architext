import { releaseStatusCanShowBlockers } from "./releaseTruth.js";

const columnDefinitions = [
  { id: "planned", label: "Planned" },
  { id: "ready", label: "Ready" },
  { id: "in-progress", label: "In Progress" },
  { id: "blocked", label: "Blocked" },
  { id: "deferred", label: "Deferred" },
  { id: "complete", label: "Complete" }
];

function releaseItems(detail) {
  if (!detail) return [];
  return [
    ...detail.scope.required,
    ...detail.scope.planned,
    ...detail.scope.stretch,
    ...detail.scope.deferred,
    ...detail.scope.outOfScope
  ];
}

function blockerItemIds(blockers) {
  return new Set(blockers
    .filter((blocker) => !blocker.status || releaseStatusCanShowBlockers(blocker.status))
    .flatMap((blocker) => blocker.itemIds));
}

function incompleteDependencyIds(items) {
  return new Set(items.filter((item) => item.status !== "complete").map((item) => item.id));
}

export function releaseKanbanColumns(detail) {
  const items = releaseItems(detail);
  const blockedItemIds = blockerItemIds(detail?.blockers ?? []);
  const incompleteDependencies = incompleteDependencyIds(items);
  const columns = new Map(columnDefinitions.map((column) => [column.id, { ...column, items: [] }]));

  for (const item of items) {
    const hasBlocker = item.status === "blocked" || (releaseStatusCanShowBlockers(item.status) && blockedItemIds.has(item.id));
    const hasUnresolvedDependency = (item.dependsOn ?? []).some((dependencyId) => incompleteDependencies.has(dependencyId));
    const columnId = kanbanColumnForItem(item.status, hasBlocker, hasUnresolvedDependency);
    columns.get(columnId)?.items.push(item);
  }

  return columnDefinitions.map((column) => columns.get(column.id));
}

function kanbanColumnForItem(status, hasBlocker, hasUnresolvedDependency) {
  if (status === "complete") return "complete";
  if (status === "deferred" || status === "cut") return "deferred";
  if (hasBlocker) return "blocked";
  if (status === "in-progress") return "in-progress";
  if (status === "stretch") return "planned";
  if (status === "planned" && !hasUnresolvedDependency) return "ready";
  return "planned";
}
