import React, { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { c4LayoutFor } from "./routing/c4Layout.js";
import { relationshipLabel } from "./routing/relationshipLabels.js";
import { plannedCanvasFallback, usePlannedDiagram } from "./routing/usePlannedDiagram.js";
import { loadArchitectureModel, loadReleaseDetail } from "./adapters/fetchArchitectureData.js";
import { isSelectedStep, orderSelectedLast, selectedFlowIdForSelection, selectedStepIdForSelection } from "./presentation/stepSelection.js";
import { diagramLayoutFor } from "./presentation/diagramLayout.js";
import { modeShowsOrderedFlow, modeUsesStructuralRelationships } from "./presentation/viewModes.js";
import { defaultViewForMode, modeForView, modeLabels, viewBelongsToMode } from "./presentation/viewSelection.js";
import { readBooleanPreference, readDebugRouting, readRoutingStylePreference, writeBooleanPreference, writeRoutingStylePreference } from "./adapters/browserPreferences.js";
import type {
  ArchNode,
  DataClass,
  Decision,
  DiagramTransform,
  Flow,
  FlowStep,
  Id,
  Mode,
  Model,
  NodeType,
  ReleaseBlocker,
  ReleaseDetail,
  ReleaseItem,
  ReleaseItemStatus,
  ReleaseModel,
  ReleaseSummary,
  Relationship,
  Risk,
  RoutingStyle,
  Selection,
  View,
  ViewportSize
} from "./domain/architectureTypes.js";
import "./styles.css";

declare const __ARCHITEXT_VERSION__: string;

const MIN_DESKTOP_FIT_ZOOM = 0.85;
const MIN_COMPACT_FIT_ZOOM = 0.7;

const statusLabels: Record<Flow["status"], string> = {
  implemented: "Implemented",
  partial: "Partial",
  planned: "Planned"
};

const routingStyleLabels: Record<RoutingStyle, string> = {
  orthogonal: "Orthogonal",
  spline: "Spline",
  straight: "Straight"
};

const releaseStatusLabels: Record<ReleaseItemStatus, string> = {
  planned: "Planned",
  "in-progress": "In Progress",
  blocked: "Blocked",
  complete: "Complete",
  deferred: "Deferred",
  stretch: "Stretch",
  cut: "Cut"
};

function releaseTone(value?: string): string {
  if (!value) return "neutral";
  if (["complete", "released", "shipped", "on-track", "low"].includes(value)) return "healthy";
  if (["planned", "planning", "active", "in-progress", "release-candidate", "stretch", "medium"].includes(value)) return "progressing";
  if (["blocked", "at-risk", "critical", "high"].includes(value)) return "blocked";
  if (["deferred", "cut"].includes(value)) return "inactive";
  return "neutral";
}

function releaseBadgeTone(value?: string): string {
  return `release-${releaseTone(value)}`;
}

function progressTone(value?: number): string {
  const progress = value ?? 0;
  if (progress <= 0) return "inactive";
  if (progress < 100) return "progressing";
  return "healthy";
}

function progressFill(value?: number): string {
  const progress = Math.max(0, Math.min(100, value ?? 0));
  if (progress <= 0) return "var(--line-strong)";
  return `color-mix(in srgb, var(--green) ${progress}%, var(--yellow))`;
}

function progressBarStyle(value?: number): React.CSSProperties {
  return { "--progress-fill": progressFill(value) } as React.CSSProperties;
}

function releaseLineState(status: ReleaseItemStatus, blocked = false): "Complete" | "Blocked" | "Deferred" | "Clear" {
  if (status === "complete") return "Complete";
  if (status === "deferred" || status === "cut") return "Deferred";
  if (blocked || status === "blocked") return "Blocked";
  return "Clear";
}

function releaseLineCheckClass(state: ReturnType<typeof releaseLineState>): string {
  if (state === "Complete") return "checked";
  return "";
}

function byId<T extends { id: Id }>(items: T[]): Map<Id, T> {
  return new Map(items.map((item) => [item.id, item]));
}

function Badge({ children, tone, title }: { children: React.ReactNode; tone?: string; title?: string }) {
  return <span className={`badge ${tone ?? ""}`} title={title}>{children}</span>;
}

function ReleaseStateBadges({ status, posture }: { status: string; posture: string }) {
  if (status === posture) {
    return (
      <Badge tone={releaseBadgeTone(status)} title={`Release status and posture are both ${status}`}>
        {status}
      </Badge>
    );
  }
  return (
    <>
      <Badge tone={releaseBadgeTone(status)} title="Release lifecycle status">Status: {status}</Badge>
      <Badge tone={releaseBadgeTone(posture)} title="Release readiness posture">Posture: {posture}</Badge>
    </>
  );
}

function scaledCanvasStyle(width: number, height: number, transform: DiagramTransform) {
  return {
    width: width * transform.zoom,
    height: height * transform.zoom
  };
}

function canvasTransformStyle(width: number, height: number, transform: DiagramTransform) {
  return {
    width,
    height,
    minWidth: width,
    minHeight: height,
    transform: `scale(${transform.zoom})`,
    transformOrigin: "0 0"
  };
}

function ScaledCanvasExtent({
  width,
  height,
  transform,
  children
}: {
  width: number;
  height: number;
  transform: DiagramTransform;
  children: React.ReactNode;
}) {
  return (
    <div className="scaled-canvas-extent" style={scaledCanvasStyle(width, height, transform)}>
      {children}
    </div>
  );
}

function useElementSize<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const [size, setSize] = useState<ViewportSize>({ width: 0, height: 0 });

  useEffect(() => {
    if (!ref.current) return;
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize({ width, height });
    });
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, []);

  return [ref, size] as const;
}

function RoutingLoadingOverlay({ active }: { active: boolean }) {
  if (!active) return null;
  return (
    <div className="routing-loading-overlay" role="status" aria-live="polite">
      <span className="routing-spinner" aria-hidden="true" />
      <span>Planning routes</span>
    </div>
  );
}

function RoutingPlanningError({ message }: { message: string }) {
  return (
    <div className="routing-planning-error" role="alert">
      <strong>Route planning failed</strong>
      <span>{message}</span>
    </div>
  );
}

function sectionId(title: string): string {
  const normalized = title.toLowerCase();
  if (normalized.includes("runtime")) return "runtime";
  if (normalized.includes("interface")) return "interfaces";
  if (normalized.includes("data")) return "data";
  if (normalized.includes("security")) return "security";
  if (normalized.includes("observability")) return "observability";
  if (normalized.includes("risk") || normalized.includes("gap")) return "risks";
  if (normalized.includes("decision")) return "decisions";
  if (normalized.includes("verification")) return "verification";
  if (normalized.includes("summary") || normalized.includes("trigger") || normalized.includes("guarantee") || normalized.includes("failure")) return "summary";
  return normalized.replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

function FieldList({ title, items }: { title: string; items: string[] }) {
  return (
    <section className="detail-section" id={sectionId(title)}>
      <h3>{title}</h3>
      {items.length > 0 ? (
        <ul>
          {items.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      ) : (
        <p className="muted">None recorded.</p>
      )}
    </section>
  );
}

function releaseItems(detail: ReleaseDetail | null): ReleaseItem[] {
  if (!detail) return [];
  return [
    ...detail.scope.required,
    ...detail.scope.planned,
    ...detail.scope.stretch,
    ...detail.scope.deferred,
    ...detail.scope.outOfScope
  ];
}

function releaseProgress(detail: ReleaseDetail | null): number {
  const required = detail?.scope.required ?? [];
  if (required.length === 0) return 0;
  const complete = required.filter((item) => item.status === "complete").length;
  return Math.round((complete / required.length) * 100);
}

function formatReleaseDate(value?: string) {
  if (!value) return "";
  return value.includes("T") ? value.slice(0, 10) : value;
}

function ReleaseTruthWorkspace({
  releases,
  activeReleaseSummary,
  activeReleaseDetail,
  selection,
  onSelectCurrentRelease,
  onSelectReleaseItem,
  onSelectReleaseMilestone
}: {
  releases?: ReleaseModel;
  activeReleaseSummary: ReleaseSummary | null;
  activeReleaseDetail: ReleaseDetail | null;
  selection: Selection | null;
  onSelectCurrentRelease: () => void;
  onSelectReleaseItem: (id: Id) => void;
  onSelectReleaseMilestone: (id: Id) => void;
}) {
  if (!releases || !activeReleaseSummary) {
    return (
      <section className="release-truth-empty">
        <h2>Release Truth</h2>
        <p>Add release data to `docs/architext/data/releases` and reference it from `manifest.files.releases`.</p>
      </section>
    );
  }

  const progress = releaseProgress(activeReleaseDetail);
  const items = releaseItems(activeReleaseDetail);
  const requiredCount = activeReleaseDetail?.scope.required.length ?? 0;
  const completeCount = items.filter((item) => item.status === "complete").length;
  const inProgressCount = items.filter((item) => item.status === "in-progress").length;
  const blockedCount = activeReleaseDetail?.blockers.length ?? activeReleaseSummary.counts.blockers;

  return (
    <section className="release-truth-workspace">
      <header className="release-hero">
        <div>
          <p className="eyebrow">Release Truth</p>
          <h2>{activeReleaseSummary.name}</h2>
          <p>{activeReleaseSummary.summary}</p>
        </div>
        <div className="release-hero-meta">
          <ReleaseStateBadges status={activeReleaseSummary.status} posture={activeReleaseSummary.posture} />
          <span>Updated {formatReleaseDate(activeReleaseSummary.lastUpdated)}</span>
          {releases.index.currentReleaseId !== activeReleaseSummary.id ? (
            <button type="button" className="button-reset release-current-button" onClick={onSelectCurrentRelease}>
              Current release
            </button>
          ) : null}
        </div>
      </header>

      <section className="release-progress-panel">
        <div className="release-progress-copy">
          <strong>{progress}% required scope complete</strong>
          <span>{requiredCount} required · {completeCount} complete · {inProgressCount} in progress · {blockedCount} blockers</span>
        </div>
        <div className={`release-progress-bar ${progressTone(progress)}`} style={progressBarStyle(progress)} aria-label={`${progress}% required scope complete`}>
          <span style={{ width: `${progress}%` }} />
        </div>
      </section>

      <div className="release-grid">
        <section className="release-section release-section-wide">
          <h3>Release Path</h3>
          {activeReleaseDetail ? (
            <ReleasePath
              detail={activeReleaseDetail}
              selection={selection}
              onSelectItem={onSelectReleaseItem}
              onSelectMilestone={onSelectReleaseMilestone}
            />
          ) : (
            <p className="muted">Release detail is loading.</p>
          )}
        </section>

        <section className="release-section release-section-wide release-history-section">
          <h3>History</h3>
          <ReleaseTrendChart releases={releases.index.releases} activeReleaseId={activeReleaseSummary.id} />
        </section>
      </div>
    </section>
  );
}

function ReleasePath({
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
        const blockedItems = milestoneItems.filter((item) => item.status === "blocked" || (blockersByItemId.get(item.id)?.length ?? 0) > 0);
        const pathNumber = milestone.status === "deferred" || milestone.status === "cut" ? 0 : milestone.order;
        return (
          <article className={`release-path-step ${releaseTone(milestone.status)}`} key={milestone.id}>
            <div className="release-path-marker">
              <span>{pathNumber}</span>
            </div>
            <div className="release-path-body">
              <ReleasePathMilestoneLine
                blockedItems={blockedItems}
                itemCount={milestoneItems.length}
                label={milestone.label}
                onSelect={() => onSelectMilestone(milestone.id)}
                selected={selection?.kind === "release-milestone" && selection.milestoneId === milestone.id}
                status={milestone.status}
                timing={milestone.date ?? milestone.targetWindow ?? "No date"}
              />
              <div className="release-path-subitems">
                {milestoneItems.length ? milestoneItems.map((item) => {
                  const workstream = item.workstreamId ? workstreamsById.get(item.workstreamId) : undefined;
                  const blockers = blockersByItemId.get(item.id) ?? [];
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
              </div>
            </div>
          </article>
        );
      })}
    </div>
  );
}

function releaseScopeByItemId(detail: ReleaseDetail): Map<Id, string> {
  return new Map([
    ...detail.scope.required.map((item) => [item.id, "required"] as const),
    ...detail.scope.planned.map((item) => [item.id, "planned"] as const),
    ...detail.scope.stretch.map((item) => [item.id, "stretch"] as const),
    ...detail.scope.deferred.map((item) => [item.id, "deferred"] as const),
    ...detail.scope.outOfScope.map((item) => [item.id, "out of scope"] as const)
  ]);
}

function ReleasePathMilestoneLine({
  blockedItems,
  itemCount,
  label,
  onSelect,
  selected,
  status,
  timing
}: {
  blockedItems: ReleaseItem[];
  itemCount: number;
  label: string;
  onSelect: () => void;
  selected: boolean;
  status: ReleaseItemStatus;
  timing: string;
}) {
  const lineState = releaseLineState(status, blockedItems.length > 0);
  const blockerText = blockedItems.map((item) => item.title).join(", ");
  return (
    <button type="button" className={`release-path-coarse-line ${selected ? "active" : ""}`} onClick={onSelect}>
      <span className={`release-check ${releaseLineCheckClass(lineState)}`} aria-label={lineState} />
      <Badge tone={releaseBadgeTone(lineState === "Blocked" ? "blocked" : status)}>{lineState}</Badge>
      <strong>{label}</strong>
      <span className="release-path-description">{releaseStatusLabels[status]} · {timing} · {itemCount} items</span>
      {blockerText ? <span className="release-path-blockers">Blocked by: {blockerText}</span> : null}
    </button>
  );
}

function blockersGroupedByItem(blockers: ReleaseBlocker[]): Map<Id, ReleaseBlocker[]> {
  const grouped = new Map<Id, ReleaseBlocker[]>();
  for (const blocker of blockers) {
    for (const itemId of blocker.itemIds) {
      grouped.set(itemId, [...(grouped.get(itemId) ?? []), blocker]);
    }
  }
  return grouped;
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
        <small>{scope} · {workstreamName} · {releaseStatusLabels[item.status]} · {item.kind}{item.priority ? ` · ${item.priority} priority` : ""}{item.owner ? ` · ${item.owner}` : ""}</small>
      </div>
      {primaryBlocker ? <span className="release-path-blockers">Blocked by: {primaryBlocker.title}</span> : null}
    </button>
  );
}

function ReleaseTrendChart({
  releases,
  activeReleaseId
}: {
  releases: ReleaseSummary[];
  activeReleaseId: Id;
}) {
  const [inspectedReleaseId, setInspectedReleaseId] = useState<Id | null>(null);
  const sorted = [...releases].sort((a, b) => (a.releasedAt ?? a.targetDate ?? a.targetWindow ?? "").localeCompare(b.releasedAt ?? b.targetDate ?? b.targetWindow ?? ""));
  const width = 1200;
  const height = 160;
  const padTop = 22;
  const padRight = 8;
  const padBottom = 46;
  const padLeft = 30;
  const baseline = height - padBottom;
  const maxCount = Math.max(1, ...sorted.flatMap((release) => [release.counts.features, release.counts.bugFixes]));
  const markerReleaseId = inspectedReleaseId ?? activeReleaseId;
  const markerIndex = Math.max(0, sorted.findIndex((release) => release.id === markerReleaseId));
  const xFor = (index: number) => sorted.length === 1 ? width / 2 : padLeft + (index * (width - padLeft - padRight)) / (sorted.length - 1);
  const yFor = (count: number) => baseline - (count * (baseline - padTop)) / maxCount;
  const pathFor = (key: "features" | "bugFixes") => sorted
    .map((release, index) => `${index === 0 ? "M" : "L"} ${xFor(index)} ${yFor(release.counts[key])}`)
    .join(" ");
  const areaFor = (key: "features" | "bugFixes") => {
    const line = sorted
      .map((release, index) => `${index === 0 ? "M" : "L"} ${xFor(index)} ${yFor(release.counts[key])}`)
      .join(" ");
    return `${line} L ${xFor(sorted.length - 1)} ${baseline} L ${xFor(0)} ${baseline} Z`;
  };
  const yTicks = Array.from(new Set([0, Math.ceil(maxCount / 2), maxCount]));

  return (
    <div className="release-history">
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Release feature and bug-fix count trend">
        <path className="release-chart-axis" d={`M ${padLeft} ${padTop} V ${baseline} H ${width - padRight}`} />
        {yTicks.map((tick) => (
          <g key={tick}>
            <path className="release-chart-tick" d={`M ${padLeft - 3} ${yFor(tick)} H ${width - padRight}`} />
            <text className="release-chart-y-label" x={padLeft - 7} y={yFor(tick) + 3} textAnchor="end">{tick}</text>
          </g>
        ))}
        <path className="release-chart-area feature" d={areaFor("features")} />
        <path className="release-chart-area fix" d={areaFor("bugFixes")} />
        <path className="release-chart-line feature" d={pathFor("features")} />
        <path className="release-chart-line fix" d={pathFor("bugFixes")} />
        <path className="release-chart-active-line" d={`M ${xFor(markerIndex)} ${padTop} V ${baseline}`} />
        {sorted.map((release, index) => (
          <g
            key={release.id}
            role="listitem"
            tabIndex={0}
            aria-label={`${release.name}, released ${formatReleaseDate(release.releasedAt ?? release.targetDate)}, ${release.counts.features} features, ${release.counts.bugFixes} bug fixes. Select it from the release list to inspect details.`}
            onClick={() => setInspectedReleaseId(release.id)}
            onFocus={() => setInspectedReleaseId(release.id)}
          >
            <title>{`${release.name} · ${formatReleaseDate(release.releasedAt ?? release.targetDate)} · ${release.counts.features} features · ${release.counts.bugFixes} bug fixes · select from the release list`}</title>
            <circle className={`${release.id === activeReleaseId ? "active" : ""} ${releaseTone(release.posture)}`} cx={xFor(index)} cy={yFor(release.counts.features)} r="3.5" />
            <text className="release-chart-x-label" x={xFor(index)} y={height - 8} textAnchor="end" transform={`rotate(-65 ${xFor(index)} ${height - 8})`}>{release.version}</text>
          </g>
        ))}
      </svg>
      <div className="release-chart-legend">
        <span><i className="feature" />Features</span>
        <span><i className="fix" />Bug fixes</span>
      </div>
    </div>
  );
}

function ReleaseTruthDetails({
  releaseSummary,
  releaseDetail,
  selection
}: {
  releaseSummary: ReleaseSummary | null;
  releaseDetail: ReleaseDetail | null;
  selection: Selection | null;
}) {
  if (!releaseSummary) {
    return (
      <div className="detail-content">
        <p className="eyebrow">Release Truth</p>
        <h2>No release selected</h2>
      </div>
    );
  }

  const items = releaseItems(releaseDetail);
  const planned = items.filter((item) => item.status === "planned");
  const stretch = items.filter((item) => item.status === "stretch");
  const active = items.filter((item) => item.status === "in-progress" || item.status === "blocked");
  const detailSelection = releasePathDetailSelection(releaseDetail, selection);

  if (detailSelection?.kind === "item") {
    const { item, blockers, dependencies, evidence, scope, workstream } = detailSelection;
    return (
      <DetailShell eyebrow="Release item" title={item.title} summary={item.summary}>
        <div className="badge-row">
          <Badge tone={releaseBadgeTone(item.status)}>{releaseStatusLabels[item.status]}</Badge>
          <Badge>{scope}</Badge>
          <Badge>{item.kind}</Badge>
          {item.priority ? <Badge tone={releaseBadgeTone(item.priority)}>{item.priority} priority</Badge> : null}
          {item.owner ? <Badge>{item.owner}</Badge> : null}
        </div>
        <FieldList title="Workstream" items={[workstream?.name ?? "Unassigned"]} />
        <FieldList title="Decision" items={[item.rationale, item.decisionSource].filter(Boolean) as string[]} />
        <FieldList title="Blockers" items={blockers.map((blocker) => `${blocker.title}: ${blocker.summary}`)} />
        <FieldList title="Next Actions" items={blockers.map((blocker) => blocker.nextAction)} />
        <FieldList title="Dependencies" items={dependencies.map((dependency) => dependency.summary)} />
        <FieldList title="Evidence" items={evidence.map((item) => item.href ? `${item.label} (${item.href})` : item.label)} />
      </DetailShell>
    );
  }

  if (detailSelection?.kind === "milestone") {
    const { milestone, items: milestoneItems, blockers } = detailSelection;
    const timing = milestone.date ?? milestone.targetWindow ?? "No date";
    return (
      <DetailShell eyebrow="Release milestone" title={milestone.label} summary={`${releaseStatusLabels[milestone.status]} · ${timing}`}>
        <div className="badge-row">
          <Badge tone={releaseBadgeTone(milestone.status)}>{releaseStatusLabels[milestone.status]}</Badge>
          <Badge>{milestoneItems.length} items</Badge>
          {blockers.length > 0 ? <Badge tone={releaseBadgeTone("blocked")}>{blockers.length} blockers</Badge> : null}
        </div>
        <FieldList title="Scope" items={milestoneItems.map((item) => `${item.title}: ${releaseStatusLabels[item.status]}`)} />
        <FieldList title="Blockers" items={blockers.map((item) => item.title)} />
      </DetailShell>
    );
  }

  return (
    <div className="detail-content">
      <p className="eyebrow">Release Truth</p>
      <h2>{releaseSummary.name}</h2>
      <p>{releaseSummary.summary}</p>
      <div className="detail-badges">
        <ReleaseStateBadges status={releaseSummary.status} posture={releaseSummary.posture} />
      </div>
      <FieldList title="Active Work" items={active.map((item) => `${item.title}: ${item.summary}`)} />
      <FieldList title="Planned Scope" items={planned.map((item) => `${item.title}: ${item.summary}`)} />
      <FieldList title="Stretch Scope" items={stretch.map((item) => `${item.title}: ${item.summary}`)} />
      <FieldList title="Evidence" items={(releaseDetail?.evidence ?? []).map((item) => item.href ? `${item.label} (${item.href})` : item.label)} />
    </div>
  );
}

function releasePathDetailSelection(detail: ReleaseDetail | null, selection: Selection | null) {
  if (!detail || !selection) return null;
  const items = releaseItems(detail);
  const itemsById = byId(items);
  const blockersByItemId = blockersGroupedByItem(detail.blockers);
  const workstreamsById = byId(detail.workstreams);
  const scopeByItemId = releaseScopeByItemId(detail);

  if (selection.kind === "release-item") {
    const item = itemsById.get(selection.itemId);
    if (!item) return null;
    const blockers = blockersByItemId.get(item.id) ?? [];
    const dependencyIds = new Set([...(item.dependsOn ?? []), ...detail.dependencies.filter((dependency) => dependency.from === item.id).map((dependency) => dependency.to)]);
    const dependencies = detail.dependencies.filter((dependency) => dependency.from === item.id || dependencyIds.has(dependency.id) || dependencyIds.has(dependency.to));
    const evidenceIds = new Set(item.evidence ?? []);
    const evidence = detail.evidence.filter((entry) => evidenceIds.has(entry.id));
    return {
      kind: "item" as const,
      item,
      blockers,
      dependencies,
      evidence,
      scope: scopeByItemId.get(item.id) ?? "scope",
      workstream: item.workstreamId ? workstreamsById.get(item.workstreamId) : undefined
    };
  }

  if (selection.kind === "release-milestone") {
    const milestone = detail.milestones.find((candidate) => candidate.id === selection.milestoneId);
    if (!milestone) return null;
    const milestoneItems = milestone.itemIds.map((itemId) => itemsById.get(itemId)).filter((item): item is ReleaseItem => Boolean(item));
    return {
      kind: "milestone" as const,
      milestone,
      items: milestoneItems,
      blockers: milestoneItems.filter((item) => item.status === "blocked" || (blockersByItemId.get(item.id)?.length ?? 0) > 0)
    };
  }

  return null;
}

function App() {
  const [model, setModel] = useState<Model | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [navCollapsed, setNavCollapsed] = useState(() => readBooleanPreference(localStorage, "architext-left-collapsed"));
  const [rightCollapsed, setRightCollapsed] = useState(() => readBooleanPreference(localStorage, "architext-right-collapsed"));
  const [query, setQuery] = useState("");
  const [activeMode, setActiveMode] = useState<Mode>("flows");
  const [activeViewId, setActiveViewId] = useState<Id>("");
  const [activeFlowId, setActiveFlowId] = useState<Id>("");
  const [activeReleaseId, setActiveReleaseId] = useState<Id>("");
  const [releaseDetailsById, setReleaseDetailsById] = useState<Map<Id, ReleaseDetail>>(new Map());
  const [selection, setSelection] = useState<Selection | null>(null);
  const [diagramTransform, setDiagramTransform] = useState<DiagramTransform>({ zoom: 1, focused: false });
  const [routingStyle, setRoutingStyle] = useState<RoutingStyle>(() => readRoutingStylePreference(localStorage) as RoutingStyle);
  const [debugRouting] = useState(() => readDebugRouting(window.location.search));
  const [riskFilter, setRiskFilter] = useState("all");
  const [stepsCollapsed, setStepsCollapsed] = useState(false);
  const [diagramViewportRef, diagramViewportSize] = useElementSize<HTMLElement>();

  useEffect(() => {
    loadArchitectureModel()
      .then((loaded: Model) => {
        setModel(loaded);
        setActiveViewId(loaded.manifest.defaultViewId);
        setActiveMode(modeForView(loaded.views.find((view) => view.id === loaded.manifest.defaultViewId)));
        setActiveFlowId(loaded.flows[0]?.id ?? "");
        setActiveReleaseId(loaded.releases?.index.currentReleaseId ?? "");
        setReleaseDetailsById(new Map((loaded.releases?.details ?? []).map((detail) => [detail.id, detail])));
        setSelection({ kind: "flow", id: loaded.flows[0]?.id ?? "" });
      })
      .catch((loadError: unknown) => {
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      });
  }, []);

  useEffect(() => {
    writeBooleanPreference(localStorage, "architext-left-collapsed", navCollapsed);
  }, [navCollapsed]);

  useEffect(() => {
    writeBooleanPreference(localStorage, "architext-right-collapsed", rightCollapsed);
  }, [rightCollapsed]);

  useEffect(() => {
    writeRoutingStylePreference(localStorage, routingStyle);
  }, [routingStyle]);

  useEffect(() => {
    const narrowWidth = window.matchMedia("(max-width: 760px)");
    const laptopWidth = window.matchMedia("(max-width: 1180px)");
    const collapseForViewport = () => {
      if (narrowWidth.matches) {
        setNavCollapsed(true);
        setRightCollapsed(true);
      } else if (laptopWidth.matches) {
        setRightCollapsed(true);
      }
    };

    collapseForViewport();
    narrowWidth.addEventListener("change", collapseForViewport);
    laptopWidth.addEventListener("change", collapseForViewport);
    return () => {
      narrowWidth.removeEventListener("change", collapseForViewport);
      laptopWidth.removeEventListener("change", collapseForViewport);
    };
  }, []);

  if (error) {
    return (
      <main className="fatal">
        <h1>Architext failed to load</h1>
        <pre>{error}</pre>
      </main>
    );
  }

  if (!model) {
    return (
      <main className="loading">
        <h1>Loading Architext</h1>
      </main>
    );
  }

  const nodesById = byId<ArchNode>(model.nodes);
  const flowsById = byId<Flow>(model.flows);
  const viewsById = byId<View>(model.views);
  const dataById = byId<DataClass>(model.dataClasses);
  const decisionsById = byId<Decision>(model.decisions);
  const risksById = byId<Risk>(model.risks);
  const activeFlow = flowsById.get(activeFlowId) ?? model.flows[0];
  const fallbackView = model.views[0];
  const selectedView = viewsById.get(activeViewId);
  const activeView = viewBelongsToMode(selectedView, activeMode)
    ? selectedView
    : defaultViewForMode(activeMode, model.views, fallbackView);
  const isC4View = activeMode === "c4";
  const isSequenceView = activeMode === "sequence";
  const isReleaseTruthView = activeMode === "release-truth";
  const showOrderedFlow = modeShowsOrderedFlow(activeMode);
  const showStructuralConnections = modeUsesStructuralRelationships(activeMode);
  const showStepSummary = showOrderedFlow;
  const flowNodeIds = new Set(activeFlow.steps.flatMap((step) => [step.from, step.to]));
  const selectedNodeId = selection?.kind === "node" ? selection.id : null;
  const selectedStepId = selectedStepIdForSelection(selection);
  const selectedFlowId = selectedFlowIdForSelection(selection);
  const selectedActiveStepId = activeFlow.steps.some((step) => isSelectedStep(selection, activeFlow.id, step.id)) ? selectedStepId : null;
  const selectedFlowForStep = selectedFlowId ? flowsById.get(selectedFlowId) : null;
  const selectedStep = selectedStepId
    ? selectedFlowForStep?.steps.find((step) => step.id === selectedStepId) ?? null
    : null;
  const activeReleaseSummary = model.releases?.index.releases.find((release) => release.id === activeReleaseId)
    ?? model.releases?.index.releases.find((release) => release.id === model.releases?.index.currentReleaseId)
    ?? null;
  const activeReleaseDetail = activeReleaseSummary ? releaseDetailsById.get(activeReleaseSummary.id) ?? null : null;

  const filteredFlows = model.flows.filter((flow) => {
    const text = [flow.name, flow.summary, flow.status, flow.trigger, ...flow.knownGaps].join(" ").toLowerCase();
    return text.includes(query.toLowerCase());
  });

  const estimateCanvasSize = (mode: Mode, view: View, flow: Flow): ViewportSize => {
    if (mode === "sequence") {
      const participantCount = new Set(flow.steps.flatMap((step) => [step.from, step.to])).size;
      return {
        width: 56 + participantCount * 146,
        height: 88 + flow.steps.length * 56 + 56
      };
    }

    if (mode === "c4") {
      return {
        width: Math.max(760, 112 + view.lanes.length * 210),
        height: Math.max(440, 72 + Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1) * 86 + 96)
      };
    }

    return {
      width: 192 + view.lanes.length * 210,
      height: Math.max(380, 86 + Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1) * 84 + 104)
    };
  };

  const fitZoomFor = (mode: Mode, view: View, flow: Flow) => {
    const estimate = estimateCanvasSize(mode, view, flow);
    const availableWidth = Math.max(diagramViewportSize.width - 24, 1);
    const availableHeight = Math.max(diagramViewportSize.height - 24, 1);
    const nextZoom = Math.min(availableWidth / estimate.width, availableHeight / estimate.height);
    const readableMinimum = window.innerWidth < 900 ? MIN_COMPACT_FIT_ZOOM : MIN_DESKTOP_FIT_ZOOM;
    return Math.min(1, Math.max(readableMinimum, Number(nextZoom.toFixed(2))));
  };

  const switchMode = (mode: Mode) => {
    const nextView = defaultViewForMode(mode, model.views, fallbackView);
    setActiveMode(mode);
    setActiveViewId(nextView.id);
    setSelection(null);
    if (mode === "release-truth" && !activeReleaseId && model.releases?.index.currentReleaseId) {
      setActiveReleaseId(model.releases.index.currentReleaseId);
    }
    if (diagramViewportSize.width && diagramViewportSize.height) {
      const nextZoom = fitZoomFor(mode, nextView, activeFlow);
      setDiagramTransform((value) => ({ ...value, zoom: Math.min(value.zoom, nextZoom) }));
    }
  };

  const selectRelease = async (releaseId: Id) => {
    setActiveReleaseId(releaseId);
    setSelection(null);
    if (!model.releases || releaseDetailsById.has(releaseId)) return;
    const detail = await loadReleaseDetail(fetch, model.releases, releaseId);
    setReleaseDetailsById((current) => new Map(current).set(detail.id, detail));
  };

  const selectCurrentRelease = () => {
    const currentReleaseId = model.releases?.index.currentReleaseId;
    if (currentReleaseId) void selectRelease(currentReleaseId);
  };

  const setC4View = (viewId: Id) => {
    setActiveMode("c4");
    setActiveViewId(viewId);
    const view = viewsById.get(viewId);
    const firstNodeId = view?.lanes.flatMap((lane) => lane.nodeIds)[0];
    if (firstNodeId) setSelection({ kind: "node", id: firstNodeId });
  };

  const selectRelationship = (relationship: Relationship) => {
    setSelection({
      kind: "relationship",
      from: relationship.from,
      to: relationship.to,
      label: relationship.label,
      relationshipType: relationship.relationshipType,
      stepId: relationship.stepId,
      flowId: relationship.flowId
    });
  };

  return (
    <div className={`app ${navCollapsed ? "left-collapsed" : ""} ${rightCollapsed ? "right-collapsed" : ""} ${diagramTransform.focused ? "diagram-focused" : ""}`}>
      <header className="topbar">
        <div>
          <p className="eyebrow">Architext / {__ARCHITEXT_VERSION__} · Data / {model.manifest.schemaVersion}</p>
          <div className="project-title-line">
            <h1>{model.manifest.project.name}</h1>
            <p>{model.manifest.project.summary}</p>
          </div>
        </div>
        <div className="topbar-actions">
          <div className="mode-tabs" role="tablist" aria-label="Architext modes">
            {(Object.keys(modeLabels) as Mode[]).map((mode) => (
              <button
                key={mode}
                type="button"
                role="tab"
                aria-selected={activeMode === mode}
                className={activeMode === mode ? "active" : ""}
                onClick={() => switchMode(mode)}
              >
                {modeLabels[mode]}
              </button>
            ))}
          </div>
        </div>
      </header>

      <aside className="left-nav">
        <button
          type="button"
          className="side-toggle left-side-toggle"
          onClick={() => setNavCollapsed((value) => !value)}
          aria-label={navCollapsed ? "Expand left navigation" : "Collapse left navigation"}
          title={navCollapsed ? "Expand left navigation" : "Collapse left navigation"}
          data-tooltip={navCollapsed ? "Expand" : "Collapse"}
        >
          {navCollapsed ? "›" : "‹"}
        </button>
        {navCollapsed ? (
          <button type="button" className="panel-rail" onClick={() => setNavCollapsed(false)}>
            Browse
          </button>
        ) : (
          <LeftPanel
            mode={activeMode}
            query={query}
            onQueryChange={setQuery}
            flows={filteredFlows}
            allFlows={model.flows}
            activeFlow={activeFlow}
            views={model.views}
            activeView={activeView}
            nodes={model.nodes}
            dataClasses={model.dataClasses}
            risks={model.risks}
            releases={model.releases}
            activeReleaseId={activeReleaseSummary?.id ?? ""}
            riskFilter={riskFilter}
            onRiskFilterChange={setRiskFilter}
            onSelectFlow={(flowId) => {
              setActiveFlowId(flowId);
              setSelection({ kind: "flow", id: flowId });
            }}
            onSelectView={setC4View}
            onSelectNode={(id) => setSelection({ kind: "node", id })}
            onSelectRelease={selectRelease}
          />
        )}
      </aside>

      <main className="diagram-area">
        {isReleaseTruthView ? (
          <ReleaseTruthWorkspace
            releases={model.releases}
            activeReleaseSummary={activeReleaseSummary}
            activeReleaseDetail={activeReleaseDetail}
            selection={selection}
            onSelectCurrentRelease={selectCurrentRelease}
            onSelectReleaseItem={(id) => {
              setSelection({ kind: "release-item", itemId: id });
              setRightCollapsed(false);
            }}
            onSelectReleaseMilestone={(id) => {
              setSelection({ kind: "release-milestone", milestoneId: id });
              setRightCollapsed(false);
            }}
          />
        ) : (
          <>
            <section className="diagram-header">
              <div className="diagram-title-line">
                <h2 title={activeView.name}>{activeView.name}</h2>
                <p title={activeView.summary}>{activeView.summary}</p>
              </div>
              <DiagramControls
                transform={diagramTransform}
                routingStyle={routingStyle}
                onRoutingStyleChange={setRoutingStyle}
                onZoomIn={() => setDiagramTransform((value) => ({ ...value, zoom: Math.min(1.6, Number((value.zoom + 0.1).toFixed(2))) }))}
                onZoomOut={() => setDiagramTransform((value) => ({ ...value, zoom: Math.max(0.7, Number((value.zoom - 0.1).toFixed(2))) }))}
                onFit={() => setDiagramTransform((value) => ({ ...value, zoom: fitZoomFor(activeMode, activeView, activeFlow) }))}
                onReset={() => setDiagramTransform((value) => ({ ...value, zoom: 1 }))}
                onToggleFocus={() => setDiagramTransform((value) => {
                  const focused = !value.focused;
                  return { ...value, focused, zoom: focused ? fitZoomFor(activeMode, activeView, activeFlow) : value.zoom };
                })}
              />
              <details className="legend">
                <summary>Legend</summary>
                <div>
                  {(["actor", "software-system", "client", "service", "worker", "queue", "data-store", "external-service"] as NodeType[]).map((type) => (
                    <span key={type}><i className={`dot ${type}`} />{type}</span>
                  ))}
                </div>
              </details>
            </section>

            <section className="diagram-viewport" ref={diagramViewportRef}>
              {isSequenceView ? (
                <SequenceDiagram
                  activeFlow={activeFlow}
                  nodesById={nodesById}
                  dataById={dataById}
                  selectedStepId={selectedActiveStepId}
                  transform={diagramTransform}
                  onSelectStep={(stepId) => setSelection({ kind: "step", flowId: activeFlow.id, stepId })}
                  onSelectRelationship={selectRelationship}
                />
              ) : isC4View ? (
                <C4Diagram
                  view={activeView}
                  nodesById={nodesById}
                  selectedNodeId={selectedNodeId}
                  selectedRelationship={selection?.kind === "relationship" ? selection : null}
                  transform={diagramTransform}
                  routingStyle={routingStyle}
                  debugRouting={debugRouting}
                  onSelectNode={(id) => setSelection({ kind: "node", id })}
                  onSelectRelationship={selectRelationship}
                />
              ) : (
                <SystemMap
                  view={activeView}
                  nodesById={nodesById}
                  activeFlow={showOrderedFlow ? activeFlow : null}
                  showStructuralConnections={showStructuralConnections}
                  selectedStepId={selectedActiveStepId}
                  selectedRelationship={selection?.kind === "relationship" ? selection : null}
                  selectedNodeId={selectedNodeId}
                  transform={diagramTransform}
                  routingStyle={routingStyle}
                  debugRouting={debugRouting}
                  onSelectNode={(id) => setSelection({ kind: "node", id })}
                  onSelectRelationship={selectRelationship}
                />
              )}
            </section>
          </>
        )}

        {!isReleaseTruthView && showStepSummary && (
          <section className={`steps ${stepsCollapsed ? "collapsed" : ""}`}>
            <div className="steps-head">
              <div className="steps-title-line">
                <h2>{activeFlow.name}</h2>
                {!stepsCollapsed && <p>{activeFlow.summary}</p>}
              </div>
              <div className="steps-actions">
                <Badge tone={activeFlow.status}>{statusLabels[activeFlow.status]}</Badge>
                <button type="button" onClick={() => setStepsCollapsed((value) => !value)}>
                  {stepsCollapsed ? "Show steps" : "Hide steps"}
                </button>
              </div>
            </div>
            {!stepsCollapsed && (
              <>
                <div className="step-list">
                  {activeFlow.steps.map((step, index) => (
                    <button
                      key={step.id}
                      type="button"
                      className={`step-card ${isSelectedStep(selection, activeFlow.id, step.id) ? "active" : ""}`}
                      onClick={() => setSelection({ kind: "step", flowId: activeFlow.id, stepId: step.id })}
                    >
                      <span className="step-number">{index + 1}</span>
                      <strong>{nodesById.get(step.from)?.name ?? step.from} {"→"} {nodesById.get(step.to)?.name ?? step.to}</strong>
                      <span>{step.action}</span>
                    </button>
                  ))}
                </div>
              </>
            )}
          </section>
        )}
      </main>

      <aside className="details">
        <button
          type="button"
          className="side-toggle right-side-toggle"
          onClick={() => setRightCollapsed((value) => !value)}
          aria-label={rightCollapsed ? "Expand right details" : "Collapse right details"}
          title={rightCollapsed ? "Expand right details" : "Collapse right details"}
          data-tooltip={rightCollapsed ? "Expand" : "Collapse"}
        >
          {rightCollapsed ? "‹" : "›"}
        </button>
        {rightCollapsed ? (
          <button type="button" className="panel-rail" onClick={() => setRightCollapsed(false)}>
            Details
          </button>
        ) : isReleaseTruthView ? (
          <ReleaseTruthDetails
            releaseSummary={activeReleaseSummary}
            releaseDetail={activeReleaseDetail}
            selection={selection}
          />
        ) : (
          <DetailPanel
            model={model}
            nodesById={nodesById}
            flowsById={flowsById}
            dataById={dataById}
            decisionsById={decisionsById}
            risksById={risksById}
            flowNodeIds={flowNodeIds}
            selection={selection}
            selectedStep={selectedStep}
            activeFlow={activeFlow}
            onSelectNode={(id) => setSelection({ kind: "node", id })}
            onSelectFlow={(id) => {
              setActiveFlowId(id);
              setSelection({ kind: "flow", id });
            }}
          />
        )}
      </aside>
    </div>
  );
}

function LeftPanel({
  mode,
  query,
  onQueryChange,
  flows,
  allFlows,
  activeFlow,
  views,
  activeView,
  nodes,
  dataClasses,
  risks,
  releases,
  activeReleaseId,
  riskFilter,
  onRiskFilterChange,
  onSelectFlow,
  onSelectView,
  onSelectNode,
  onSelectRelease
}: {
  mode: Mode;
  query: string;
  onQueryChange: (value: string) => void;
  flows: Flow[];
  allFlows: Flow[];
  activeFlow: Flow;
  views: View[];
  activeView: View;
  nodes: ArchNode[];
  dataClasses: DataClass[];
  risks: Risk[];
  releases?: ReleaseModel;
  activeReleaseId: Id;
  riskFilter: string;
  onRiskFilterChange: (value: string) => void;
  onSelectFlow: (id: Id) => void;
  onSelectView: (id: Id) => void;
  onSelectNode: (id: Id) => void;
  onSelectRelease: (id: Id) => void;
}) {
  if (mode === "release-truth") {
    return (
      <>
        <div className="panel-head">
          <h2>Release Truth</h2>
          <p>{releases ? `${releases.index.releases.length} tracked releases` : "No release data configured."}</p>
        </div>
        <div className="entity-list">
          {releases ? [...releases.index.releases]
            .sort((a, b) => (b.releasedAt ?? b.targetDate ?? b.targetWindow ?? "").localeCompare(a.releasedAt ?? a.targetDate ?? a.targetWindow ?? ""))
            .map((release) => (
            <button
              key={release.id}
              type="button"
              className={`entity-card ${activeReleaseId === release.id ? "active" : ""}`}
              onClick={() => onSelectRelease(release.id)}
            >
              <strong>{release.name}</strong>
              <span>{release.summary}</span>
              <div className="release-card-badges">
                <ReleaseStateBadges status={release.status} posture={release.posture} />
              </div>
            </button>
          )) : (
            <article className="entity-card passive">
              <strong>No Release Truth data</strong>
              <span>Add docs/architext/data/releases/index.json and reference it from manifest.files.releases.</span>
            </article>
          )}
        </div>
      </>
    );
  }

  if (mode === "c4") {
    const c4Views = views.filter((view) => view.type.startsWith("c4-"));
    return (
      <>
        <div className="panel-head">
          <h2>C4 Drilldown</h2>
          <p>Structural levels, not workflows.</p>
        </div>
        <div className="entity-list">
          {c4Views.map((view) => (
            <button
              key={view.id}
              type="button"
              className={`entity-card ${activeView.id === view.id ? "active" : ""}`}
              onClick={() => onSelectView(view.id)}
            >
              <strong>{view.name.replace("C4 ", "")}</strong>
              <span>{view.summary}</span>
            </button>
          ))}
        </div>
      </>
    );
  }

  if (mode === "deployment") {
    const deploymentNodes = nodes.filter((node) => ["client", "service", "worker", "queue", "data-store", "external-service", "deployment-unit"].includes(node.type));
    return (
      <>
        <div className="panel-head">
          <h2>Runtime Units</h2>
          <p>{deploymentNodes.length} nodes in deployment scope.</p>
        </div>
        <div className="entity-list">
          {deploymentNodes.map((node) => (
            <button key={node.id} type="button" className="entity-card" onClick={() => onSelectNode(node.id)}>
              <strong>{node.name}</strong>
              <span>{node.runtime}</span>
              <Badge>{node.type}</Badge>
            </button>
          ))}
        </div>
      </>
    );
  }

  if (mode === "data-risks") {
    const riskTones = ["all", "critical", "high", "medium", "low"];
    const normalizedQuery = query.toLowerCase();
    const filteredDataClasses = dataClasses.filter((item) => (
      [item.name, item.handling, item.sensitivity].join(" ").toLowerCase().includes(normalizedQuery)
    ));
    const filteredRisks = risks.filter((risk) => {
      const matchesText = [risk.title, risk.summary, risk.severity].join(" ").toLowerCase().includes(normalizedQuery);
      const matchesTone = riskFilter === "all" || risk.severity === riskFilter;
      return matchesText && matchesTone;
    });
    return (
      <>
        <div className="panel-head">
          <h2>Data / Risks</h2>
          <input
            type="search"
            value={query}
            placeholder="Filter data or risks"
            aria-label="Filter data or risks"
            onChange={(event) => onQueryChange(event.target.value)}
          />
          <div className="filter-row" aria-label="Risk severity filters">
            {riskTones.map((tone) => (
              <button
                key={tone}
                type="button"
                className={riskFilter === tone ? "active" : ""}
                onClick={() => onRiskFilterChange(tone)}
              >
                {tone}
              </button>
            ))}
          </div>
          <p>{dataClasses.length} data classes · {risks.length} risks</p>
        </div>
        <div className="entity-list">
          <h3>Data Classes</h3>
          {filteredDataClasses.map((item) => (
            <article className="entity-card passive" key={item.id}>
              <strong>{item.name}</strong>
              <span>{item.handling}</span>
              <Badge tone={item.sensitivity}>{item.sensitivity}</Badge>
            </article>
          ))}
          <h3>Risks</h3>
          {filteredRisks.map((risk) => (
            <article className="entity-card passive" key={risk.id}>
              <strong>{risk.title}</strong>
              <span>{risk.summary}</span>
              <Badge tone={risk.severity}>{risk.severity}</Badge>
            </article>
          ))}
        </div>
      </>
    );
  }

  return (
    <>
      <div className="panel-head">
        <h2>{mode === "sequence" ? "Sequence Flow" : "Flows"}</h2>
        <input
          type="search"
          value={query}
          placeholder="Search flows"
          aria-label="Search flows"
          onChange={(event) => onQueryChange(event.target.value)}
        />
        <p>{flows.length} of {allFlows.length} flows</p>
      </div>
      <div className="flow-list">
        {flows.map((flow) => (
          <button
            key={flow.id}
            type="button"
            className={`flow-card ${flow.id === activeFlow.id ? "active" : ""}`}
            onClick={() => onSelectFlow(flow.id)}
          >
            <strong>{flow.name}</strong>
            <span>{flow.summary}</span>
            <Badge tone={flow.status}>{statusLabels[flow.status]}</Badge>
          </button>
        ))}
      </div>
    </>
  );
}

function DiagramControls({
  transform,
  routingStyle,
  onRoutingStyleChange,
  onZoomIn,
  onZoomOut,
  onFit,
  onReset,
  onToggleFocus
}: {
  transform: DiagramTransform;
  routingStyle: RoutingStyle;
  onRoutingStyleChange: (style: RoutingStyle) => void;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFit: () => void;
  onReset: () => void;
  onToggleFocus: () => void;
}) {
  return (
    <div className="diagram-controls" aria-label="Diagram controls">
      <label className="routing-style-control">
        <span>Line Style</span>
        <select
          value={routingStyle}
          aria-label="Line Style"
          onChange={(event) => onRoutingStyleChange(event.target.value as RoutingStyle)}
        >
          {(Object.keys(routingStyleLabels) as RoutingStyle[]).map((style) => (
            <option key={style} value={style}>{routingStyleLabels[style]}</option>
          ))}
        </select>
      </label>
      <button type="button" onClick={onZoomOut} aria-label="Zoom out">-</button>
      <span>{Math.round(transform.zoom * 100)}%</span>
      <button type="button" onClick={onZoomIn} aria-label="Zoom in">+</button>
      <button type="button" onClick={onFit}>Fit</button>
      <button type="button" onClick={onReset}>Reset</button>
      <button type="button" onClick={onToggleFocus}>{transform.focused ? "Exit focus" : "Focus"}</button>
    </div>
  );
}

function SystemMap({
  view,
  nodesById,
  activeFlow,
  showStructuralConnections,
  selectedStepId,
  selectedRelationship,
  selectedNodeId,
  transform,
  routingStyle,
  debugRouting,
  onSelectRelationship,
  onSelectNode
}: {
  view: View;
  nodesById: Map<Id, ArchNode>;
  activeFlow: Flow | null;
  showStructuralConnections: boolean;
  selectedStepId: Id | null;
  selectedRelationship: Extract<Selection, { kind: "relationship" }> | null;
  selectedNodeId: Id | null;
  transform: DiagramTransform;
  routingStyle: RoutingStyle;
  debugRouting: boolean;
  onSelectRelationship: (relationship: Relationship) => void;
  onSelectNode: (id: Id) => void;
}) {
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  const flowNodeIds = new Set(activeFlow ? activeFlow.steps.flatMap((step) => [step.from, step.to]) : Array.from(visibleNodeIds));
  const structuralRelationships = Array.from(visibleNodeIds).flatMap((nodeId) => {
    const node = nodesById.get(nodeId);
    return (node?.dependencies ?? [])
      .filter((dependencyId) => visibleNodeIds.has(dependencyId))
      .map((dependencyId) => {
        const to = nodesById.get(dependencyId);
        const label = relationshipLabel(node, to);
        return {
          id: `${nodeId}-${dependencyId}`,
          from: nodeId,
          to: dependencyId,
          label,
          summary: `${node?.name ?? nodeId} ${label} ${to?.name ?? dependencyId}`,
          relationshipType: "structural" as const,
          toType: to?.type
        };
      });
  });

  const flowRelationships = activeFlow?.steps.map((step, index) => {
    const from = nodesById.get(step.from);
    const to = nodesById.get(step.to);
    return {
      id: step.id,
      from: step.from,
      to: step.to,
      label: `${index + 1}. ${step.action}`,
      summary: step.summary,
      relationshipType: "flow" as const,
      stepId: step.id,
      flowId: activeFlow.id,
      displayIndex: index + 1
    };
  }) ?? [];

  const layout = diagramLayoutFor(view, showStructuralConnections ? structuralRelationships.length : flowRelationships.length);
  const {
    nodeWidth,
    nodeHeight,
    laneWidth,
    rowGap,
    marginX,
    marginY,
    minCanvasWidth,
    minCanvasHeight,
    canvasExtraWidth,
    canvasExtraHeight
  } = layout;

  const planInput = {
    view,
    relationships: showStructuralConnections ? structuralRelationships : flowRelationships,
    visibleNodeIds,
    nodeWidth,
    nodeHeight,
    laneWidth,
    rowGap,
    marginX,
    marginY,
    minCanvasWidth,
    minCanvasHeight,
    canvasExtraWidth,
    canvasExtraHeight,
    style: routingStyle
  };
  const planningState = usePlannedDiagram(planInput);
  const fallbackCanvas = plannedCanvasFallback(planInput);
  const plan = planningState.plan;

  if (planningState.error) {
    return (
      <section className="map-shell">
        <ScaledCanvasExtent width={fallbackCanvas.width} height={fallbackCanvas.height} transform={transform}>
          <div
            className="diagram-canvas"
            style={canvasTransformStyle(fallbackCanvas.width, fallbackCanvas.height, transform)}
          >
            <RoutingPlanningError message={planningState.error} />
          </div>
        </ScaledCanvasExtent>
      </section>
    );
  }

  if (!plan) {
    return (
      <section className="map-shell" aria-busy={planningState.planning ? "true" : "false"}>
        <ScaledCanvasExtent width={fallbackCanvas.width} height={fallbackCanvas.height} transform={transform}>
          <div
            className="diagram-canvas"
            style={canvasTransformStyle(fallbackCanvas.width, fallbackCanvas.height, transform)}
          >
            <RoutingLoadingOverlay active={planningState.planning} />
          </div>
        </ScaledCanvasExtent>
      </section>
    );
  }

  const canvasWidth = plan.canvasWidth;
  const canvasHeight = plan.canvasHeight;
  const structuralRoutes = showStructuralConnections ? plan.routes : new Map();
  const flowRoutes = showStructuralConnections ? new Map() : plan.routes;
  const nodePosition = plan.positionFor;
  const isStructuralSelected = (relationship: Relationship) => (
    selectedRelationship?.from === relationship.from && selectedRelationship.to === relationship.to
  );
  const isFlowSelected = (relationship: Relationship) => (
    selectedStepId === relationship.stepId || (
      selectedRelationship?.from === relationship.from &&
      selectedRelationship.to === relationship.to &&
      selectedRelationship.stepId === relationship.stepId
    )
  );
  const orderedStructuralRelationships = [...structuralRelationships].sort((a, b) => Number(isStructuralSelected(a)) - Number(isStructuralSelected(b)));
  const orderedFlowRelationships = [...flowRelationships].sort((a, b) => Number(isFlowSelected(a)) - Number(isFlowSelected(b)));

  return (
    <section className="map-shell" aria-busy={planningState.planning ? "true" : "false"}>
      <ScaledCanvasExtent width={canvasWidth} height={canvasHeight} transform={transform}>
        <div
          className="diagram-canvas"
          style={canvasTransformStyle(canvasWidth, canvasHeight, transform)}
        >
          <svg className="flow-lines" width={canvasWidth} height={canvasHeight} aria-hidden="false" role="group" aria-label={`${view.name} relationships`}>
          <defs>
            <marker id="arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="arrowhead-selected" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="flow-arrowhead-selected" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
          </defs>
          {showStructuralConnections && orderedStructuralRelationships.map((connection, index) => {
            const route = structuralRoutes.get(connection.id);
            if (!route) return null;
            const selected = isStructuralSelected(connection);
            return (
              <g
                key={`${connection.from}-${connection.to}`}
                className={selected ? "relationship-edge selected" : "relationship-edge"}
                role="button"
                tabIndex={0}
                aria-label={connection.summary}
                onClick={() => onSelectRelationship(connection)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") onSelectRelationship(connection);
                }}
              >
                <path
                  className="structural-line"
                  d={route.d}
                  markerEnd={selected ? "url(#arrowhead-selected)" : "url(#arrowhead)"}
                />
                <text className="relationship-label" x={route.labelX} y={route.labelY - 8}>{connection.label}</text>
              </g>
            );
          })}
          {!showStructuralConnections && orderedFlowRelationships.map((relationship, index) => {
            if (!plan.laneIndexByNode.has(relationship.from) || !plan.laneIndexByNode.has(relationship.to)) {
              return null;
            }
            const route = flowRoutes.get(relationship.id);
            if (!route) return null;
            const isSelected = isFlowSelected(relationship);
            return (
              <g
                key={relationship.id}
                className={isSelected ? "flow-edge selected" : "flow-edge"}
                role="button"
                tabIndex={0}
                aria-label={relationship.summary}
                onClick={() => onSelectRelationship(relationship)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") onSelectRelationship(relationship);
                }}
              >
                <path
                  className="flow-line"
                  d={route.d}
                  markerEnd={isSelected ? "url(#flow-arrowhead-selected)" : "url(#arrowhead)"}
                />
                <rect className="route-step-marker flow-step-dot" x={route.labelX - 12} y={route.labelY - 10} width="24" height="20" rx="10" />
                <text className="route-step-label flow-step-label" x={route.labelX} y={route.labelY + 4}>{relationship.displayIndex}</text>
              </g>
            );
          })}
          {debugRouting ? <RoutingDebugGeometry plan={plan} relationships={showStructuralConnections ? structuralRelationships : flowRelationships} /> : null}
          </svg>
          {view.lanes.map((lane, laneIndex) => (
            <div
              className="lane-column"
              key={lane.id}
              style={{ left: marginX + laneIndex * laneWidth, width: nodeWidth, height: canvasHeight - 20 }}
            >
              <h3>{lane.name}</h3>
            </div>
          ))}
          {view.lanes.flatMap((lane) => lane.nodeIds).map((nodeId) => {
            const node = nodesById.get(nodeId);
            if (!node) return null;
            const isActive = flowNodeIds.has(node.id);
            const isSelected = selectedNodeId === node.id;
            const position = nodePosition(node.id);
            return (
              <button
                key={node.id}
                type="button"
                className={`node-card ${node.type} ${isActive ? "in-flow" : ""} ${isSelected ? "selected" : ""}`}
                style={{ left: position.x, top: position.y, width: nodeWidth, height: nodeHeight }}
                onClick={() => onSelectNode(node.id)}
              >
                <strong>{node.name}</strong>
                <span>{node.type}</span>
              </button>
            );
          })}
          <RoutingLoadingOverlay active={planningState.planning} />
        </div>
      </ScaledCanvasExtent>
      {!activeFlow ? (
        <div className="edge-strip">
          <span className="edge-count">Structural connections only</span>
        </div>
      ) : null}
      {debugRouting ? (
        <RoutingDebugPanel
          plan={plan}
          relationships={showStructuralConnections ? structuralRelationships : flowRelationships}
        />
      ) : null}
    </section>
  );
}

function topQualityCosts(route: any) {
  return Object.entries(route.qualityCosts ?? {})
    .filter(([, value]) => typeof value === "number" && value !== 0)
    .sort(([, left], [, right]) => Math.abs(right as number) - Math.abs(left as number))
    .slice(0, 5);
}

function RoutingDebugPanel({ plan, relationships }: { plan: any; relationships: Relationship[] }) {
  const warnings = plan.warnings ?? [];
  return (
    <div className="routing-debug-panel" aria-label="Routing debug data">
      <div className="routing-debug-summary">
        <strong>Routing debug</strong>
        <span>{plan.routes.size} routes</span>
        <span>{warnings.length} warnings</span>
      </div>
      {warnings.length > 0 ? (
        <div className="routing-debug-warnings">
          {warnings.slice(0, 8).map((warning: any, index: number) => (
            <span key={`${warning.relationshipId ?? warning.nodeId ?? warning.viewId ?? "diagram"}-${warning.code}-${index}`}>
              {warning.relationshipId ?? warning.nodeId ?? warning.viewId ?? "diagram"}: {warning.code}
            </span>
          ))}
        </div>
      ) : null}
      <div className="routing-debug-routes">
        {relationships.map((relationship) => {
          const route = plan.routes.get(relationship.id);
          if (!route) return null;
          return (
            <details key={relationship.id}>
              <summary>
                <span>{relationship.id}</span>
                <span>{Math.round(route.cost)}</span>
              </summary>
              <dl>
                {topQualityCosts(route).map(([name, value]) => (
                  <React.Fragment key={name}>
                    <dt>{name}</dt>
                    <dd>{Math.round(value as number)}</dd>
                  </React.Fragment>
                ))}
              </dl>
            </details>
          );
        })}
      </div>
    </div>
  );
}

function RoutingDebugGeometry({ plan, relationships }: { plan: any; relationships: Relationship[] }) {
  return (
    <g className="routing-debug-geometry" aria-hidden="true">
      {[...plan.nodeRects.entries()].map(([nodeId, rect]: [string, any]) => (
        <rect
          key={`node-${nodeId}`}
          className="routing-debug-node"
          x={rect.x}
          y={rect.y}
          width={rect.width}
          height={rect.height}
        />
      ))}
      {relationships.map((relationship) => {
        const route = plan.routes.get(relationship.id);
        const labelBox = plan.labelBoxes.get(relationship.id);
        if (!route) return null;
        return (
          <g key={`debug-${relationship.id}`} className={route.warnings?.length ? "routing-debug-route warned" : "routing-debug-route"}>
            {labelBox ? (
              <rect
                className="routing-debug-label"
                x={labelBox.x}
                y={labelBox.y}
                width={labelBox.width}
                height={labelBox.height}
              />
            ) : null}
            {route.points?.map((point: any, index: number) => (
              <circle
                key={`point-${relationship.id}-${index}`}
                className="routing-debug-point"
                cx={point.x}
                cy={point.y}
                r={index === 0 || index === route.points.length - 1 ? 4 : 2.5}
              />
            ))}
          </g>
        );
      })}
    </g>
  );
}

function C4Diagram({
  view,
  nodesById,
  selectedNodeId,
  selectedRelationship,
  transform,
  routingStyle,
  debugRouting,
  onSelectNode,
  onSelectRelationship
}: {
  view: View;
  nodesById: Map<Id, ArchNode>;
  selectedNodeId: Id | null;
  selectedRelationship: Extract<Selection, { kind: "relationship" }> | null;
  transform: DiagramTransform;
  routingStyle: RoutingStyle;
  debugRouting: boolean;
  onSelectNode: (id: Id) => void;
  onSelectRelationship: (relationship: Relationship) => void;
}) {
  const layout = c4LayoutFor(view.type);
  const {
    nodeWidth,
    nodeHeight,
    laneWidth,
    rowGap,
    marginX,
    marginY,
    minCanvasWidth,
    minCanvasHeight,
    canvasExtraWidth,
    canvasExtraHeight,
    boundaryLabel
  } = layout;
  const rawNodeIds = view.lanes.flatMap((lane) => lane.nodeIds);
  const allNodeIds = Array.from(new Set(rawNodeIds));
  const duplicateNodeIds = Array.from(
    rawNodeIds.reduce((counts, nodeId) => counts.set(nodeId, (counts.get(nodeId) ?? 0) + 1), new Map<Id, number>())
  ).filter(([, count]) => count > 1).map(([nodeId]) => nodeId);
  const documentWarnings = duplicateNodeIds.map((nodeId) => ({
    code: "duplicate-c4-node",
    nodeId,
    viewId: view.id,
    message: `${nodeId} appears more than once in ${view.name}; rendered once.`
  }));
  const visibleNodeIds = new Set(allNodeIds);
  const relationships = allNodeIds.flatMap((nodeId) => {
    const node = nodesById.get(nodeId);
    return (node?.dependencies ?? [])
      .filter((dependencyId) => visibleNodeIds.has(dependencyId))
      .map((dependencyId) => {
        const to = nodesById.get(dependencyId);
        const label = relationshipLabel(node, to);
        return {
          id: `${nodeId}-${dependencyId}`,
          from: nodeId,
          to: dependencyId,
          label,
          summary: `${node?.name ?? nodeId} ${label} ${to?.name ?? dependencyId}`,
          relationshipType: "structural" as const,
          toType: to?.type
        };
      });
  });
  const planInput = {
    view,
    relationships,
    visibleNodeIds,
    nodeWidth,
    nodeHeight,
    laneWidth,
    rowGap,
    marginX,
    marginY,
    minCanvasWidth,
    minCanvasHeight,
    canvasExtraWidth,
    canvasExtraHeight,
    style: routingStyle
  };
  const planningState = usePlannedDiagram(planInput);
  const fallbackCanvas = plannedCanvasFallback(planInput);
  const plan = planningState.plan;

  if (planningState.error) {
    return (
      <section className="map-shell c4-shell">
        <ScaledCanvasExtent width={fallbackCanvas.width} height={fallbackCanvas.height} transform={transform}>
          <div
            className={`c4-canvas ${view.type}`}
            style={canvasTransformStyle(fallbackCanvas.width, fallbackCanvas.height, transform)}
          >
            <RoutingPlanningError message={planningState.error} />
          </div>
        </ScaledCanvasExtent>
      </section>
    );
  }

  if (!plan) {
    return (
      <section className="map-shell c4-shell" aria-busy={planningState.planning ? "true" : "false"}>
        <ScaledCanvasExtent width={fallbackCanvas.width} height={fallbackCanvas.height} transform={transform}>
          <div
            className={`c4-canvas ${view.type}`}
            style={canvasTransformStyle(fallbackCanvas.width, fallbackCanvas.height, transform)}
          >
            <RoutingLoadingOverlay active={planningState.planning} />
          </div>
        </ScaledCanvasExtent>
      </section>
    );
  }

  const canvasWidth = plan.canvasWidth;
  const canvasHeight = plan.canvasHeight;
  const debugPlan = documentWarnings.length ? { ...plan, warnings: [...documentWarnings, ...(plan.warnings ?? [])] } : plan;
  const positionFor = plan.positionFor;
  const isC4RelationshipSelected = (relationship: Relationship) => (
    selectedRelationship?.from === relationship.from && selectedRelationship.to === relationship.to
  );
  const orderedRelationships = [...relationships].sort((a, b) => Number(isC4RelationshipSelected(a)) - Number(isC4RelationshipSelected(b)));

  return (
    <section className="map-shell c4-shell" aria-busy={planningState.planning ? "true" : "false"}>
      <ScaledCanvasExtent width={canvasWidth} height={canvasHeight} transform={transform}>
        <div
          className={`c4-canvas ${view.type}`}
          style={canvasTransformStyle(canvasWidth, canvasHeight, transform)}
        >
          <svg className="flow-lines c4-lines" width={canvasWidth} height={canvasHeight} role="group" aria-label={`${view.name} structural relationships`}>
          <defs>
            <marker id="c4-arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="c4-arrowhead-selected" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
          </defs>
          {orderedRelationships.map((relationship) => {
            const route = plan.routes.get(relationship.id);
            if (!route) return null;
            const selected = isC4RelationshipSelected(relationship);
            return (
              <g
                key={relationship.id}
                className={selected ? "relationship-edge selected" : "relationship-edge"}
                role="button"
                tabIndex={0}
                aria-label={relationship.summary}
                onClick={() => onSelectRelationship(relationship)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") onSelectRelationship(relationship);
                }}
              >
                <path
                  className={`c4-relationship target-${relationship.toType ?? "unknown"}`}
                  d={route.d}
                  markerEnd={selected ? "url(#c4-arrowhead-selected)" : "url(#c4-arrowhead)"}
                />
                <text className="relationship-label c4-label" x={route.labelX} y={route.labelY}>{relationship.label}</text>
              </g>
            );
          })}
          {debugRouting ? <RoutingDebugGeometry plan={plan} relationships={relationships} /> : null}
          </svg>
          <div className={`c4-boundary ${view.type}`}>
            <span>{boundaryLabel}</span>
          </div>
          {view.lanes.map((lane, laneIndex) => (
            <div
              className="c4-lane-label"
              key={lane.id}
              style={{ left: marginX + laneIndex * laneWidth, top: 34, width: nodeWidth }}
            >
              {lane.name}
            </div>
          ))}
          {allNodeIds.map((nodeId) => {
            const node = nodesById.get(nodeId);
            if (!node) return null;
            const position = positionFor(nodeId);
            return (
              <button
                key={node.id}
                type="button"
                className={`c4-node ${node.type} ${selectedNodeId === node.id ? "selected" : ""}`}
                style={{ left: position.x, top: position.y, width: nodeWidth, height: nodeHeight }}
                onClick={() => onSelectNode(node.id)}
                aria-label={`${node.name}, ${node.type}. ${node.summary}`}
              >
                <strong>{node.name}</strong>
                <span>{node.type}</span>
                <small>{node.summary}</small>
              </button>
            );
          })}
          <RoutingLoadingOverlay active={planningState.planning} />
        </div>
      </ScaledCanvasExtent>
      <div className="edge-strip">
        <span className="edge-count">{relationships.length} labeled structural relationships</span>
      </div>
      {debugRouting ? <RoutingDebugPanel plan={debugPlan} relationships={relationships} /> : null}
    </section>
  );
}

function SequenceDiagram({
  activeFlow,
  nodesById,
  dataById,
  selectedStepId,
  transform,
  onSelectRelationship,
  onSelectStep
}: {
  activeFlow: Flow;
  nodesById: Map<Id, ArchNode>;
  dataById: Map<Id, DataClass>;
  selectedStepId: Id | null;
  transform: DiagramTransform;
  onSelectRelationship: (relationship: Relationship) => void;
  onSelectStep: (stepId: Id) => void;
}) {
  const participantIds = Array.from(new Set(activeFlow.steps.flatMap((step) => [step.from, step.to])));
  const participantWidth = 146;
  const rowHeight = 56;
  const marginX = 28;
  const headerY = 18;
  const messageStartY = 68;
  const width = marginX * 2 + participantIds.length * participantWidth;
  const height = messageStartY + activeFlow.steps.length * rowHeight + 38;
  const xFor = (id: Id) => marginX + participantIds.indexOf(id) * participantWidth + participantWidth / 2;
  const orderedStepMessages: { step: FlowStep; index: number }[] = orderSelectedLast(
    activeFlow.steps.map((step, index) => ({ step, index })),
    ({ step }: { step: FlowStep }) => selectedStepId === step.id
  );

  return (
    <section className="map-shell sequence-shell">
      <ScaledCanvasExtent width={width} height={height} transform={transform}>
        <div
          className="sequence-participant-rail"
          style={{ width, transform: `scale(${transform.zoom})`, transformOrigin: "0 0" }}
        >
          {participantIds.map((id) => {
            const node = nodesById.get(id);
            const x = xFor(id);
            return (
              <button
                key={id}
                type="button"
                className={`sequence-participant-card ${node?.type ?? ""}`}
                style={{ left: x - 58, width: 116 }}
                aria-label={`${node?.name ?? id} participant`}
              >
                <strong>{node?.name ?? id}</strong>
                <span>{node?.type ?? "node"}</span>
              </button>
            );
          })}
        </div>
        <svg
          className="sequence-canvas"
          width={width}
          height={height}
          role="img"
          aria-label={`${activeFlow.name} sequence diagram`}
          style={canvasTransformStyle(width, height, transform)}
        >
          <defs>
            <marker id="sequence-arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="sequence-arrowhead-response" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="sequence-arrowhead-persistence" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="sequence-arrowhead-selected" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
          </defs>
          {participantIds.map((id) => {
            const node = nodesById.get(id);
            const x = xFor(id);
            return (
              <g key={id}>
                <line className="lifeline" x1={x} y1={headerY + 48} x2={x} y2={height - 22} />
              </g>
            );
          })}
          {orderedStepMessages.map(({ step, index }) => {
            const fromX = xFor(step.from);
            const toX = xFor(step.to);
            const y = messageStartY + index * rowHeight;
            const midX = (fromX + toX) / 2;
            const dataLabel = step.data.map((id) => dataById.get(id)?.name ?? id).join(", ");
            const messageKind = step.to.includes("queue") ? "async" : step.to.includes("db") || step.to.includes("store") ? "persistence" : toX < fromX ? "response" : "request";
            const markerId = selectedStepId === step.id
              ? "sequence-arrowhead-selected"
              : messageKind === "response"
                ? "sequence-arrowhead-response"
                : messageKind === "persistence"
                  ? "sequence-arrowhead-persistence"
                  : "sequence-arrowhead";
            const relationship = {
              id: step.id,
              from: step.from,
              to: step.to,
              label: step.action,
              summary: step.summary,
              relationshipType: "flow" as const,
              stepId: step.id,
              flowId: activeFlow.id
            };
            return (
              <g
                key={step.id}
                className={`sequence-message ${messageKind} ${selectedStepId === step.id ? "selected" : ""}`}
                role="button"
                tabIndex={0}
                aria-label={`${step.action}: ${step.summary}`}
                onClick={() => {
                  onSelectStep(step.id);
                  onSelectRelationship(relationship);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    onSelectStep(step.id);
                    onSelectRelationship(relationship);
                  }
                }}
              >
                <line
                  className="sequence-line"
                  x1={fromX}
                  y1={y}
                  x2={toX}
                  y2={y}
                  markerEnd={`url(#${markerId})`}
                />
                <rect className="route-step-marker sequence-step-dot" x={midX - 12} y={y - 10} width="24" height="20" rx="10" />
                <text className="route-step-label sequence-step-label" x={midX} y={y + 4}>{index + 1}</text>
                <text className="sequence-action" x={midX} y={y - 17}>{step.action.length > 26 ? `${step.action.slice(0, 23)}...` : step.action}</text>
                <text className="sequence-data" x={midX} y={y + 30}>{dataLabel}</text>
              </g>
            );
          })}
        </svg>
      </ScaledCanvasExtent>
    </section>
  );
}

function DetailPanel({
  nodesById,
  flowsById,
  dataById,
  decisionsById,
  risksById,
  selection,
  selectedStep,
  activeFlow,
  onSelectNode,
  onSelectFlow
}: {
  model: Model;
  nodesById: Map<Id, ArchNode>;
  flowsById: Map<Id, Flow>;
  dataById: Map<Id, DataClass>;
  decisionsById: Map<Id, Decision>;
  risksById: Map<Id, Risk>;
  flowNodeIds: Set<Id>;
  selection: Selection | null;
  selectedStep: FlowStep | null | undefined;
  activeFlow: Flow;
  onSelectNode: (id: Id) => void;
  onSelectFlow: (id: Id) => void;
}) {
  if (selection?.kind === "relationship") {
    const from = nodesById.get(selection.from);
    const to = nodesById.get(selection.to);
    const relatedStep = selection.flowId && selection.stepId
      ? flowsById.get(selection.flowId)?.steps.find((step) => step.id === selection.stepId)
      : null;
    const flow = selection.flowId ? flowsById.get(selection.flowId) : null;
    return (
      <DetailShell eyebrow="Relationship" title={`${from?.name ?? selection.from} → ${to?.name ?? selection.to}`} summary={selection.label}>
        <div className="badge-row">
          <Badge>{selection.relationshipType}</Badge>
          {flow && <Badge>{flow.name}</Badge>}
        </div>
        <FieldList title="Summary" items={[relatedStep?.summary ?? `${from?.name ?? selection.from} ${selection.label} ${to?.name ?? selection.to}`]} />
        <section className="detail-section" id="runtime">
          <h3>Endpoints</h3>
          <div className="path-pair">
            <button type="button" onClick={() => onSelectNode(selection.from)}>{from?.name ?? selection.from}</button>
            <span>{"→"}</span>
            <button type="button" onClick={() => onSelectNode(selection.to)}>{to?.name ?? selection.to}</button>
          </div>
        </section>
        {relatedStep && (
          <FieldList title="Data" items={relatedStep.data.map((id) => dataById.get(id)?.name ?? id)} />
        )}
      </DetailShell>
    );
  }

  if (selection?.kind === "node") {
    const node = nodesById.get(selection.id);
    if (!node) return <EmptyDetail />;
    const relatedFlows = node.relatedFlows.map((id) => flowsById.get(id)).filter(Boolean) as Flow[];
    const decisions = node.relatedDecisions.map((id) => decisionsById.get(id)).filter(Boolean) as Decision[];
    const risks = node.knownRisks.map((id) => risksById.get(id)).filter(Boolean) as Risk[];
    const dataClasses = node.dataHandled.map((id) => dataById.get(id)).filter(Boolean) as DataClass[];

    return (
      <DetailShell eyebrow={node.type} title={node.name} summary={node.summary}>
        <div className="badge-row">
          <Badge>{node.owner}</Badge>
          {dataClasses.map((item) => <Badge key={item.id} tone={item.sensitivity}>{item.name}</Badge>)}
        </div>
        <FieldList title="Responsibilities" items={node.responsibilities} />
        <FieldList title="Source paths" items={node.sourcePaths} />
        <FieldList title="Runtime / deployment" items={[node.runtime]} />
        <FieldList title="Interfaces" items={node.interfaces} />
        <FieldList title="Dependencies" items={node.dependencies.map((id) => nodesById.get(id)?.name ?? id)} />
        <FieldList title="Security / trust" items={node.security} />
        <FieldList title="Observability" items={node.observability} />
        <LinkList title="Related flows" items={relatedFlows.map((flow) => ({ id: flow.id, label: flow.name }))} onClick={onSelectFlow} />
        <DecisionList decisions={decisions} />
        <RiskList risks={risks} />
        <FieldList title="Verification" items={node.verification} />
      </DetailShell>
    );
  }

  if (selection?.kind === "step" && selectedStep) {
    const from = nodesById.get(selectedStep.from);
    const to = nodesById.get(selectedStep.to);
    const dataClasses = selectedStep.data.map((id) => dataById.get(id)).filter(Boolean) as DataClass[];
    return (
      <DetailShell eyebrow="Flow step" title={selectedStep.action} summary={selectedStep.summary}>
        <div className="path-pair">
          <button type="button" onClick={() => onSelectNode(selectedStep.from)}>{from?.name ?? selectedStep.from}</button>
          <span>{"→"}</span>
          <button type="button" onClick={() => onSelectNode(selectedStep.to)}>{to?.name ?? selectedStep.to}</button>
        </div>
        <section className="detail-section">
          <h3>Data moved</h3>
          {dataClasses.map((item) => (
            <article className="data-class" key={item.id}>
              <strong>{item.name}</strong>
              <Badge tone={item.sensitivity}>{item.sensitivity}</Badge>
              <p>{item.handling}</p>
            </article>
          ))}
        </section>
      </DetailShell>
    );
  }

  const flow = selection?.kind === "flow" ? flowsById.get(selection.id) ?? activeFlow : activeFlow;
  return (
    <DetailShell eyebrow="Flow" title={flow.name} summary={flow.summary}>
      <div className="badge-row">
        <Badge tone={flow.status}>{statusLabels[flow.status]}</Badge>
        <Badge>{flow.steps.length} steps</Badge>
      </div>
      <FieldList title="Trigger" items={[flow.trigger]} />
      <FieldList title="Guarantees" items={flow.guarantees} />
      <FieldList title="Failure behavior" items={flow.failureBehavior} />
      <FieldList title="Observability" items={flow.observability} />
      <FieldList title="Known gaps" items={flow.knownGaps} />
      <FieldList title="Verification" items={flow.verification} />
    </DetailShell>
  );
}

function DetailShell({ eyebrow, title, summary, children }: { eyebrow: string; title: string; summary: string; children: React.ReactNode }) {
  const sections = ["Summary", "Runtime", "Interfaces", "Data", "Security", "Observability", "Risks", "Decisions", "Verification"];
  return (
    <div className="detail-content">
      <div className="detail-sticky">
        <p className="eyebrow">{eyebrow}</p>
        <h2>{title}</h2>
        <p>{summary}</p>
        <nav className="detail-index" aria-label="Detail sections">
          {sections.map((section) => (
            <a key={section} href={`#${section.toLowerCase().replaceAll(" ", "-")}`}>{section}</a>
          ))}
        </nav>
      </div>
      {children}
    </div>
  );
}

function LinkList({ title, items, onClick }: { title: string; items: Array<{ id: Id; label: string }>; onClick: (id: Id) => void }) {
  return (
    <section className="detail-section">
      <h3>{title}</h3>
      {items.length > 0 ? items.map((item) => (
        <button className="text-link" type="button" key={item.id} onClick={() => onClick(item.id)}>
          {item.label}
        </button>
      )) : <p className="muted">None recorded.</p>}
    </section>
  );
}

function DecisionList({ decisions }: { decisions: Decision[] }) {
  return (
    <section className="detail-section" id="decisions">
      <h3>Related decisions</h3>
      {decisions.length > 0 ? decisions.map((decision) => (
        <article className="mini-record" key={decision.id}>
          <strong>{decision.title}</strong>
          <p>{decision.decision}</p>
        </article>
      )) : <p className="muted">None recorded.</p>}
    </section>
  );
}

function RiskList({ risks }: { risks: Risk[] }) {
  return (
    <section className="detail-section" id="risks">
      <h3>Known risks</h3>
      {risks.length > 0 ? risks.map((risk) => (
        <article className="mini-record" key={risk.id}>
          <strong>{risk.title}</strong>
          <Badge tone={risk.severity}>{risk.severity}</Badge>
          <p>{risk.summary}</p>
        </article>
      )) : <p className="muted">None recorded.</p>}
    </section>
  );
}

function EmptyDetail() {
  return (
    <div className="detail-content">
      <h2>No selection</h2>
      <p>Select a node, flow, or step to inspect details.</p>
    </div>
  );
}

type ArchitextWindow = Window & { __architextRoot?: Root };
type ArchitextHotData = { root?: Root };
type ArchitextImportMeta = ImportMeta & { hot?: { data: ArchitextHotData } };
type ArchitextRootElement = HTMLElement & { __architextRoot?: Root };

const rootElement = document.getElementById("root") as ArchitextRootElement;
const hot = (import.meta as ArchitextImportMeta).hot;
const hotData = hot?.data;
const existingRoot = rootElement.__architextRoot ?? hotData?.root ?? (window as ArchitextWindow).__architextRoot;
const root = existingRoot ?? createRoot(rootElement);

rootElement.__architextRoot = root;
(window as ArchitextWindow).__architextRoot = root;
if (hot) {
  hot.data.root = root;
}

root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
