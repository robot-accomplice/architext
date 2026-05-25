import { useState } from "react";
import type { Id, ReleaseBlocker, ReleaseDetail, ReleaseItem, ReleaseItemStatus, Selection } from "../domain/architectureTypes.js";
import { Badge } from "./Badge.js";
import { toggleCollapsedReleasePathMilestone } from "./releasePathState.js";
import {
  activeReleaseBlockersForItem,
  blockersGroupedByItem,
  releaseBadgeTone,
  releaseItemSummaryText,
  releaseItems,
  releaseLineCheckClass,
  releaseLineState,
  releasePathCompletionText,
  releaseScopeByItemId,
  releaseStatusLabels,
  releaseTone
} from "./releaseTruth.js";

function byId<T extends { id: Id }>(items: T[]): Map<Id, T> {
  return new Map(items.map((item) => [item.id, item]));
}

export function ReleasePath({
  detail,
  selection,
  onSelectItem,
  onSelectMilestone
}: {
  detail: ReleaseDetail;
  selection: Selection | null;
  onSelectItem: (id: Id) => void;
  onSelectMilestone: (id: Id) => void;
}) {
  const [collapsedMilestoneIds, setCollapsedMilestoneIds] = useState<Set<Id>>(() => new Set());
  const allItems = releaseItems(detail);
  const itemsById = byId(allItems);
  const workstreamsById = byId(detail.workstreams);
  const blockersByItemId = blockersGroupedByItem(detail.blockers);
  const linkedItemIds = new Set(detail.milestones.flatMap((milestone) => milestone.itemIds));
  const unlinkedItems = allItems.filter((item) => !linkedItemIds.has(item.id));
  const milestones = [
    ...detail.milestones,
    ...(unlinkedItems.length > 0 ? [{
      id: "unlinked-release-scope",
      label: "Other considered release scope",
      status: "planned" as ReleaseItemStatus,
      date: undefined,
      targetWindow: "Tracked outside explicit milestones",
      order: Math.max(0, ...detail.milestones.map((milestone) => milestone.order)) + 1,
      itemIds: unlinkedItems.map((item) => item.id)
    }] : [])
  ].sort((a, b) => a.order - b.order);
  const scopeByItemId = releaseScopeByItemId(detail);

  return (
    <div className="release-path">
      {milestones.map((milestone) => {
        const milestoneItems = milestone.itemIds.map((itemId) => itemsById.get(itemId)).filter((item): item is ReleaseItem => Boolean(item));
        const blockedItems = milestoneItems.filter((item) => item.status === "blocked" || activeReleaseBlockersForItem(item, blockersByItemId.get(item.id) ?? []).length > 0);
        const collapsed = collapsedMilestoneIds.has(milestone.id);
        const pathNumber = milestone.status === "deferred" || milestone.status === "cut" ? 0 : milestone.order;
        return (
          <article className={`release-path-step ${releaseTone(milestone.status)} ${collapsed ? "collapsed" : ""}`} key={milestone.id}>
            <div className="release-path-marker">
              <span>{pathNumber}</span>
            </div>
            <div className="release-path-body">
              <ReleasePathMilestoneLine
                blockedItems={blockedItems}
                collapsed={collapsed}
                completionText={releasePathCompletionText(milestoneItems)}
                itemCount={milestoneItems.length}
                label={milestone.label}
                onSelect={() => onSelectMilestone(milestone.id)}
                onToggleCollapsed={() => setCollapsedMilestoneIds((current) => toggleCollapsedReleasePathMilestone(current, milestone.id))}
                selected={selection?.kind === "release-milestone" && selection.milestoneId === milestone.id}
                status={milestone.status}
                timing={milestone.date ?? milestone.targetWindow ?? "No date"}
              />
              {collapsed ? null : <div className="release-path-subitems">
                {milestoneItems.length ? milestoneItems.map((item) => {
                  const workstream = item.workstreamId ? workstreamsById.get(item.workstreamId) : undefined;
                  const blockers = activeReleaseBlockersForItem(item, blockersByItemId.get(item.id) ?? []);
                  return (
                    <ReleasePathItem
                      blockers={blockers}
                      item={item}
                      key={item.id}
                      onSelect={() => onSelectItem(item.id)}
                      selected={selection?.kind === "release-item" && selection.itemId === item.id}
                      scope={scopeByItemId.get(item.id) ?? "scope"}
                      workstreamName={workstream?.name ?? "Unassigned"}
                    />
                  );
                }) : (
                  <p className="muted">No linked release items.</p>
                )}
              </div>}
            </div>
          </article>
        );
      })}
    </div>
  );
}

function ReleasePathMilestoneLine({
  blockedItems,
  collapsed,
  completionText,
  itemCount,
  label,
  onSelect,
  onToggleCollapsed,
  selected,
  status,
  timing
}: {
  blockedItems: ReleaseItem[];
  collapsed: boolean;
  completionText: string;
  itemCount: number;
  label: string;
  onSelect: () => void;
  onToggleCollapsed: () => void;
  selected: boolean;
  status: ReleaseItemStatus;
  timing: string;
}) {
  const lineState = releaseLineState(status, blockedItems.length > 0);
  const blockerText = blockedItems.map((item) => item.title).join(", ");
  return (
    <div className="release-path-coarse-line">
      <button
        type="button"
        className="release-path-collapse"
        aria-expanded={!collapsed}
        aria-label={`${collapsed ? "Expand" : "Collapse"} ${label}`}
        onClick={(event) => {
          event.stopPropagation();
          onToggleCollapsed();
        }}
      >
        {collapsed ? "+" : "-"}
      </button>
      <button type="button" className={`release-path-milestone-select ${selected ? "active" : ""}`} onClick={onSelect}>
        <span className={`release-check ${releaseLineCheckClass(lineState)}`} aria-label={lineState} />
        <Badge tone={releaseBadgeTone(lineState === "Blocked" ? "blocked" : status)}>{lineState}</Badge>
        <strong>{label}</strong>
        <span className="release-path-description">{releaseStatusLabels[status]} · {timing} · {completionText} · {itemCount} items</span>
        {blockerText ? <span className="release-path-blockers">Blocked by: {blockerText}</span> : null}
      </button>
    </div>
  );
}

function ReleasePathItem({
  item,
  workstreamName,
  blockers,
  onSelect,
  selected,
  scope
}: {
  item: ReleaseItem;
  workstreamName: string;
  blockers: ReleaseBlocker[];
  onSelect: () => void;
  selected: boolean;
  scope: string;
}) {
  const primaryBlocker = blockers[0];
  const lineTone = primaryBlocker ? releaseTone(primaryBlocker.severity) : releaseTone(item.status);
  const state = releaseLineState(item.status, Boolean(primaryBlocker));
  return (
    <button type="button" className={`release-path-line release-path-item ${lineTone} ${selected ? "active" : ""}`} onClick={onSelect}>
      <span className={`release-check ${releaseLineCheckClass(state)}`} aria-label={state} />
      <Badge tone={releaseBadgeTone(state === "Blocked" ? primaryBlocker?.severity ?? "blocked" : item.status)}>{state}</Badge>
      <div className="release-path-line-main">
        <strong>{item.title}</strong>
        <span className="release-path-item-summary">{releaseItemSummaryText(item)}</span>
        <small>{scope} · {workstreamName} · {releaseStatusLabels[item.status]} · {item.kind}{item.priority ? ` · ${item.priority} priority` : ""}{item.owner ? ` · ${item.owner}` : ""}</small>
      </div>
      {primaryBlocker ? <span className="release-path-blockers">Blocked by: {primaryBlocker.title}</span> : null}
    </button>
  );
}
