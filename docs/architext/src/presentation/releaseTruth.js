export const releaseStatusLabels = {
  planned: "Planned",
  "in-progress": "In Progress",
  blocked: "Blocked",
  complete: "Complete",
  deferred: "Deferred",
  stretch: "Stretch",
  cut: "Cut"
};

export function releaseItems(detail) {
  if (!detail) return [];
  return [
    ...detail.scope.required,
    ...detail.scope.planned,
    ...detail.scope.stretch,
    ...detail.scope.deferred,
    ...detail.scope.outOfScope
  ];
}

export function releaseItemSummaryText(item) {
  return item.summary ?? "";
}

export function releaseProgress(detail) {
  const required = detail?.scope.required ?? [];
  if (required.length === 0) return 0;
  const complete = required.filter((item) => item.status === "complete").length;
  return Math.round((complete / required.length) * 100);
}

export function releaseTone(value) {
  if (!value) return "neutral";
  if (["complete", "completed", "shipped", "on-track", "low"].includes(value)) return "healthy";
  if (["draft", "planned", "implementing", "in-progress", "release-candidate", "stretch", "medium"].includes(value)) return "progressing";
  if (["blocked", "at-risk", "critical", "high"].includes(value)) return "blocked";
  if (["deferred", "cut"].includes(value)) return "inactive";
  return "neutral";
}

export function releaseBadgeTone(value) {
  return `release-${releaseTone(value)}`;
}

export function progressTone(value) {
  const progress = value ?? 0;
  if (progress <= 0) return "inactive";
  if (progress < 100) return "progressing";
  return "healthy";
}

export function progressFill(value) {
  const progress = Math.max(0, Math.min(100, value ?? 0));
  if (progress <= 0) return "var(--line-strong)";
  return `color-mix(in srgb, var(--green) ${progress}%, var(--yellow))`;
}

export function formatReleaseDate(value) {
  if (!value) return "";
  return value.includes("T") ? value.slice(0, 10) : value;
}

export function releaseLineState(status, blocked = false) {
  if (status === "complete") return "Complete";
  if (status === "deferred" || status === "cut") return "Deferred";
  if (blocked || status === "blocked") return "Blocked";
  return "Not Blocked";
}

export function releaseStatusCanShowBlockers(status) {
  return !["complete", "deferred", "cut"].includes(status);
}

export function activeReleaseBlockersForItem(item, blockers = []) {
  if (!releaseStatusCanShowBlockers(item.status)) return [];
  return blockers.filter((blocker) => !blocker.status || releaseStatusCanShowBlockers(blocker.status));
}

export function releaseLineCheckClass(state) {
  if (state === "Complete") return "checked";
  return "";
}

export function releaseScopeByItemId(detail) {
  return new Map([
    ...detail.scope.required.map((item) => [item.id, "required"]),
    ...detail.scope.planned.map((item) => [item.id, "planned"]),
    ...detail.scope.stretch.map((item) => [item.id, "stretch"]),
    ...detail.scope.deferred.map((item) => [item.id, "deferred"]),
    ...detail.scope.outOfScope.map((item) => [item.id, "out of scope"])
  ]);
}
