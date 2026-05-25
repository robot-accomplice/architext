export function releasePlanActionDisabled({ pending, version, selectedCount }) {
  return Boolean(pending) || !version.trim() || selectedCount === 0;
}

export function releasePlanProposalPayload({
  dryRun,
  action,
  version,
  theme,
  selectedRoadmapIds,
  itemScopes,
  adHocItems
}) {
  return {
    dryRun,
    action,
    version,
    theme: theme.trim(),
    selectedRoadmapItemIds: selectedRoadmapIds,
    itemScopes,
    adHocItems: adHocItems.map(({ id, persisted, ...item }) => ({
      ...(persisted ? { id } : {}),
      ...item
    }))
  };
}

export function dataRefreshNoticeForDirtyEditors({ releasePlanningDirty, rulesEditorDirty }) {
  if (!releasePlanningDirty && !rulesEditorDirty) return null;
  return "Architext data changed. Save or discard editor changes before refreshing.";
}
