export function toggleCollapsedReleasePathMilestone(collapsedMilestoneIds, milestoneId) {
  const nextCollapsedMilestoneIds = new Set(collapsedMilestoneIds);
  if (nextCollapsedMilestoneIds.has(milestoneId)) {
    nextCollapsedMilestoneIds.delete(milestoneId);
  } else {
    nextCollapsedMilestoneIds.add(milestoneId);
  }
  return nextCollapsedMilestoneIds;
}

export function collapseAllReleasePathMilestones(milestoneIds) {
  return new Set(milestoneIds);
}

export function expandAllReleasePathMilestones() {
  return new Set();
}
