import React, { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { c4LayoutFor } from "./routing/c4Layout.js";
import { pathToSvgWithHops } from "./routing/routeEdges.js";
import { relationshipLabel } from "./routing/relationshipLabels.js";
import { plannedCanvasFallback, usePlannedDiagram } from "./routing/usePlannedDiagram.js";
import {
  loadArchitectureModel,
  loadReleaseDetail,
  releaseDetailsForSelectedRelease,
  selectedReleaseIdForReload
} from "./adapters/fetchArchitectureData.js";
import { mutationFetch } from "./adapters/mutationAuth.js";
import { isSelectedStep, orderSelectedLast, selectedFlowIdForSelection, selectedStepIdForSelection } from "./presentation/stepSelection.js";
import { Badge } from "./presentation/Badge.js";
import { diagramLayoutFor } from "./presentation/diagramLayout.js";
import { c4DrilldownUnavailableReason, childC4ViewForNode } from "./presentation/c4Drilldown.js";
import { ReleaseKanban } from "./presentation/ReleaseKanbanView.js";
import { ReleasePath } from "./presentation/ReleasePathView.js";
import { ReleasePlanningPanel, ReleasePlanningWorkspace } from "./presentation/ReleasePlanning.js";
import { ReleaseTrendChart } from "./presentation/ReleaseTrendChart.js";
import { nextRuleCategoryName, orderedRules, ruleCategories, ruleCategoryAccent, ruleCriticalityTone, ruleProtectionLabel } from "./presentation/rules.js";
import { DiagramIcon } from "./presentation/DiagramIcon.js";
import { iconForNodeType, iconForStep } from "./presentation/diagramIconModel.js";
import { decisionBranchTargets, flowStepDisplayIndexes, isDecisionBranchSupportStep } from "./presentation/flowStepDisplayModel.js";
import { postRulesAction } from "./presentation/rulesClient.js";
import { postNotesAction } from "./presentation/notesClient.js";
import { NotesSection } from "./presentation/NotesSection.js";
import { useUnsavedEditorGuard } from "./presentation/unsavedEditorGuard.js";
import { useArchitextModel, type RecoveryResult } from "./presentation/useArchitextModel.js";
import { useDiagramViewport } from "./presentation/useDiagramViewport.js";
import { DiagramConfigContext, useDiagramConfig, type DiagramConfig } from "./presentation/diagramConfigContext.js";
import { fetchDiagramConfig } from "./adapters/fetchDiagramConfig.js";
import { fetchRepoTree } from "./adapters/fetchRepoTree.js";
import { RepoTreeWorkspace } from "./presentation/RepoTreeWorkspace.js";
import { ownerLegend } from "./presentation/repoTreeColors.js";
import { BlastRadiusWorkspace } from "./presentation/BlastRadiusWorkspace.js";
import { searchRepository, blastRadiusForNode } from "./presentation/blastRadius.js";
import { DiagramConfigPanel, type DiagramFieldsSpec, type DiagramSectionLabels } from "./presentation/DiagramConfigPanel.js";
import { DIAGRAM_FIELD_SPEC, DIAGRAM_SECTION_LABELS } from "./presentation/diagramFieldSpec.js";
import { nodeLanePosition, preferredDecisionBranchSide, preferredDecisionBranchEndSide } from "./presentation/decisionBranchModel.js";
import { postDiagramConfig } from "./presentation/diagramConfigClient.js";
import { pdfExportControlLabel, requestPdfExport } from "./presentation/pdfExportModel.js";
import { StepRoute } from "./presentation/StepRoute.js";
import { sequenceActivationSpans, sequenceStepMessageKind, stepRouteClassName } from "./presentation/stepRouteModel.js";
import {
  progressFill,
  progressTone,
  activeReleaseBlockersForItem,
  blockersGroupedByItem,
  formatReleaseDate,
  releaseBadgeTone,
  releaseItems,
  releaseLineState,
  releaseProgress,
  releaseScopeByItemId,
  releaseStatusLabels
} from "./presentation/releaseTruth.js";
import { modeShowsOrderedFlow, modeUsesStructuralRelationships } from "./presentation/viewModes.js";
import {
  compatibleFlowsForView,
  compatibleFlowViewsForFlow,
  defaultFlowForView,
  defaultViewForFlow,
  defaultViewForMode,
  hashForMode,
  modeForHash,
  modeForView,
  modeLabels,
  viewBelongsToMode
} from "./presentation/viewSelection.js";
import type {
  ArchNode,
  DataClass,
  Decision,
  DiagramTransform,
  ElementNote,
  Flow,
  FlowStep,
  Id,
  Mode,
  Model,
  NodeType,
  ReleaseDetail,
  ReleaseItem,
  ReleaseModel,
  ReleaseSummary,
  Relationship,
  RuleItem,
  RoadmapItem,
  Risk,
  RoutingStyle,
  Selection,
  View,
} from "./domain/architectureTypes.js";
import "./styles.css";

declare const __ARCHITEXT_VERSION__: string;

type RouteSide = NonNullable<Relationship["preferredStartSide"]>;
type DiagramPoint = { x: number; y: number };
type DiagramRect = { x: number; y: number; width: number; height: number };
type DiagramRoute = {
  id?: string;
  points?: DiagramPoint[];
  samples?: DiagramPoint[];
  labelX: number;
  labelY: number;
  cost: number;
  qualityCosts?: Record<string, number>;
  warnings?: unknown[];
};
type DiagramPlan = {
  warnings?: Array<{ code?: string; message?: string; relationshipId?: string; otherRelationshipId?: string; nodeId?: string; viewId?: string }>;
  nodeRects: Map<string, DiagramRect>;
  routes: Map<string, DiagramRoute>;
  labelBoxes: Map<string, DiagramRect>;
};

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

type SequenceActivationSpan = {
  id: Id;
  participantId: Id;
  y1: number;
  y2: number;
  depth: number;
};

function progressBarStyle(value?: number): React.CSSProperties {
  return { "--progress-fill": progressFill(value) } as React.CSSProperties;
}

function outcomeLabelPoint(route: DiagramRoute) {
  const start = route.points?.[0] ?? { x: route.labelX, y: route.labelY };
  const samples = route.samples?.length ? route.samples : route.points ?? [start];
  const anchor = samples.find((sample) => Math.hypot(sample.x - start.x, sample.y - start.y) >= 92)
    ?? route.points?.[1]
    ?? route.points?.at?.(-1)
    ?? { x: route.labelX, y: route.labelY };
  return {
    anchorX: anchor.x,
    anchorY: anchor.y,
    x: anchor.x,
    y: anchor.y
  };
}

function decisionNodeId(stepId: Id) {
  return `decision:${stepId}`;
}

function decisionTip(rect: { x: number; y: number; width: number; height: number }, side: RouteSide) {
  const center = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  const radius = rect.width / Math.SQRT2;
  if (side === "left") return { x: center.x - radius, y: center.y };
  if (side === "right") return { x: center.x + radius, y: center.y };
  if (side === "top") return { x: center.x, y: center.y - radius };
  return { x: center.x, y: center.y + radius };
}

function decisionRouteRect(rect: { x: number; y: number; width: number; height: number }) {
  return {
    ...rect,
    fixedPorts: true,
    sideAnchors: {
      left: decisionTip(rect, "left"),
      right: decisionTip(rect, "right"),
      top: decisionTip(rect, "top"),
      bottom: decisionTip(rect, "bottom")
    }
  };
}

// Connector from the affiliated node down to the diamond's node-facing (top) tip.
// The diamond sits below its node, so the TOP point is the node side; branches
// only ever use the other three tips (left/right/bottom), so they never collide
// with this connection.
function decisionConnectorRoute(decisionNode: { componentId: Id; rect: { x: number; y: number; width: number; height: number } }, componentRect: { x: number; y: number; width: number; height: number }) {
  const x = componentRect.x + componentRect.width / 2;
  const decisionTop = decisionTip(decisionNode.rect, "top");
  return {
    points: [
      { x, y: componentRect.y + componentRect.height },
      decisionTop
    ]
  };
}


function byId<T extends { id: Id }>(items: T[]): Map<Id, T> {
  return new Map(items.map((item) => [item.id, item]));
}

function releaseStateLabel(value: string) {
  return value.replaceAll("-", " ").toUpperCase();
}

function ReleaseStateBadges({ status, posture }: { status: string; posture: string }) {
  if (status === posture) {
    return (
      <Badge tone={releaseBadgeTone(status)} title={`Release status and posture are both ${status}`}>
        {releaseStateLabel(status)}
      </Badge>
    );
  }
  return (
    <>
      <Badge tone={releaseBadgeTone(status)} title="Release lifecycle status">Status: {releaseStateLabel(status)}</Badge>
      <Badge tone={releaseBadgeTone(posture)} title="Release readiness posture">Posture: {releaseStateLabel(posture)}</Badge>
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

// The drawn content (nodes + routes) usually fills less than the full canvas — the layout adds
// outer margins and the sparsest lane can leave a wide empty band. Fit should frame the CONTENT,
// not that whitespace, so we measure the content's extent and expose it for the fit calculation.
function contentExtent(plan: { nodeRects: Map<string, { x: number; y: number; width: number; height: number }>; routes: Map<string, { points: { x: number; y: number }[] }>; canvasWidth: number; canvasHeight: number }) {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const rect of plan.nodeRects.values()) {
    minX = Math.min(minX, rect.x);
    minY = Math.min(minY, rect.y);
    maxX = Math.max(maxX, rect.x + rect.width);
    maxY = Math.max(maxY, rect.y + rect.height);
  }
  for (const route of plan.routes.values()) {
    for (const point of route.points) {
      minX = Math.min(minX, point.x);
      minY = Math.min(minY, point.y);
      maxX = Math.max(maxX, point.x);
      maxY = Math.max(maxY, point.y);
    }
  }
  if (!Number.isFinite(minX)) return { width: plan.canvasWidth, height: plan.canvasHeight };
  const PAD = 24; // breathing room so content is not flush against the viewport edge
  return {
    width: Math.min(plan.canvasWidth, maxX - minX + PAD * 2),
    height: Math.min(plan.canvasHeight, maxY - minY + PAD * 2)
  };
}

function ScaledCanvasExtent({
  width,
  height,
  transform,
  contentWidth = width,
  contentHeight = height,
  children
}: {
  width: number;
  height: number;
  transform: DiagramTransform;
  contentWidth?: number;
  contentHeight?: number;
  children: React.ReactNode;
}) {
  return (
    <div
      className="scaled-canvas-extent"
      data-canvas-width={width}
      data-canvas-height={height}
      data-content-width={contentWidth}
      data-content-height={contentHeight}
      style={scaledCanvasStyle(width, height, transform)}
    >
      {children}
    </div>
  );
}

// Centered transient/notice overlays must sit on the VIEWPORT, not inside the zoom-transformed
// `.diagram-canvas` (a CSS transform makes the ancestor the containing block for position:fixed,
// so an in-canvas overlay scales and pans with the diagram). Portalling to document.body frees the
// overlay from that transform; the fixed, viewport-centred placement lives in CSS.
function ViewportOverlay({
  className,
  children,
  ...rest
}: { className: string; children: React.ReactNode } & React.HTMLAttributes<HTMLDivElement>) {
  if (typeof document === "undefined") return null;
  return createPortal(
    <div className={className} {...rest}>
      {children}
    </div>,
    document.body
  );
}

type PlanningProgress = { label: string; done: number; total: number; routesConsidered: number };

function RoutingLoadingOverlay({ active, phase, progress }: { active: boolean; phase?: string; progress?: PlanningProgress | null }) {
  // Elapsed timer: hooks must run unconditionally (before the early return) to
  // keep React's hook order stable when `active` toggles.
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  useEffect(() => {
    if (!active) {
      setElapsedSeconds(0);
      return undefined;
    }
    const startedAt = Date.now();
    const timer = window.setInterval(() => setElapsedSeconds(Math.round((Date.now() - startedAt) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
  if (!active) return null;
  const label = progress?.label || phase || "Planning routes";
  const detail: string[] = [];
  if (progress && progress.total > 0) detail.push(`${progress.done}/${progress.total} edges`);
  if (progress && progress.routesConsidered > 0) detail.push(`${progress.routesConsidered.toLocaleString()} routes considered`);
  if (elapsedSeconds >= 2) detail.push(`${elapsedSeconds}s`);
  return (
    <ViewportOverlay className="routing-loading-overlay" role="status" aria-live="polite">
      <span className="routing-spinner" aria-hidden="true" />
      <span>{label}…</span>
      {detail.length > 0 ? <span className="routing-progress-detail">{detail.join(" · ")}</span> : null}
    </ViewportOverlay>
  );
}

function RoutingPlanningError({ message }: { message: string }) {
  return (
    <ViewportOverlay className="routing-planning-error" role="alert">
      <strong>Route planning failed</strong>
      <span>{message}</span>
    </ViewportOverlay>
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

function OptionalFieldList({ title, items }: { title: string; items: string[] }) {
  if (items.length === 0) return null;
  return <FieldList title={title} items={items} />;
}

function slugifyRuleId(value: string) {
  return value.toLowerCase().trim().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "new-rule";
}

function editableRuleSnapshot(rule: RuleItem | null) {
  if (!rule) return null;
  return JSON.stringify({
    id: rule.id,
    title: rule.title,
    summary: rule.summary,
    category: rule.category,
    criticality: rule.criticality,
    order: rule.order,
    source: rule.source,
    protection: rule.protection
  });
}

function RulesWorkspace({
  rules,
  selectedRule,
  selectedCategory,
  newRuleDraftRequest,
  onSelectRule,
  onRuleSaved,
  onRulesChanged,
  onEditingChange
}: {
  rules: RuleItem[];
  selectedRule: RuleItem | null;
  selectedCategory: string;
  newRuleDraftRequest: { id: number; category: string } | null;
  onSelectRule: (id: Id) => void;
  onRuleSaved: (id: Id) => void;
  onRulesChanged: () => Promise<void>;
  onEditingChange: (editing: boolean) => void;
}) {
  const ordered = orderedRules(rules);
  const visibleRules = selectedCategory === "all"
    ? ordered
    : ordered.filter((rule) => rule.category === selectedCategory);
  const selectedCategoryLabel = selectedCategory === "all" ? "All Rules" : selectedCategory;
  const [draft, setDraft] = useState<RuleItem | null>(selectedRule);
  const [message, setMessage] = useState("");
  const [pendingAction, setPendingAction] = useState(false);
  const [dragRuleId, setDragRuleId] = useState<Id | null>(null);
  const draftIsNew = Boolean(draft && !rules.some((rule) => rule.id === draft.id));
  const draftEditProtected = !draftIsNew && selectedRule?.protection.edit;
  const draftCannotMove = !selectedRule || draftIsNew || selectedRule.protection.edit || selectedRule.protection.delete;
  const draftCannotDelete = !selectedRule || draftIsNew || selectedRule.protection.delete;
  const draftDirty = Boolean(draft) && (
    draftIsNew || editableRuleSnapshot(draft) !== editableRuleSnapshot(selectedRule)
  );

  useEffect(() => {
    setDraft(selectedRule);
    setMessage("");
  }, [selectedRule]);

  useEffect(() => {
    onEditingChange(draftDirty);
  }, [draftDirty, onEditingChange]);

  useEffect(() => () => onEditingChange(false), [onEditingChange]);

  const sendRuleAction = async (payload: object) => {
    if (pendingAction) return;
    setMessage("");
    setPendingAction(true);
    try {
      await postRulesAction(mutationFetch, payload);
      await onRulesChanged();
      setMessage("Rules updated.");
    } finally {
      setPendingAction(false);
    }
  };

  const createRuleDraft = (category = selectedCategory === "all" ? "Architecture" : selectedCategory) => {
    const existingIds = new Set(rules.map((rule) => rule.id));
    let id = "new-rule";
    let suffix = 2;
    while (existingIds.has(id)) {
      id = `new-rule-${suffix}`;
      suffix += 1;
    }
    setDraft({
      id,
      title: "New rule",
      summary: "Describe the rule.",
      category,
      criticality: "medium",
      order: Math.max(0, ...rules.map((rule) => rule.order)) + 10,
      source: "agent",
      protection: { edit: false, delete: false }
    });
    setMessage("");
  };

  useEffect(() => {
    if (!newRuleDraftRequest) return;
    createRuleDraft(newRuleDraftRequest.category);
  }, [newRuleDraftRequest?.id]);

  const saveDraft = async () => {
    if (!draft) return;
    try {
      await sendRuleAction({ action: "update", rule: draft });
      onEditingChange(false);
      onRuleSaved(draft.id);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const deleteSelectedRule = async () => {
    if (!selectedRule || draftCannotDelete) return;
    try {
      await sendRuleAction({ action: "delete", id: selectedRule.id });
      setDraft(null);
      onEditingChange(false);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const moveSelectedRule = async (direction: "up" | "down") => {
    if (!selectedRule || draftCannotMove) return;
    try {
      await sendRuleAction({ action: "move", id: selectedRule.id, direction });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const dropRuleBefore = async (beforeId: Id) => {
    if (!dragRuleId || dragRuleId === beforeId) return;
    try {
      await sendRuleAction({ action: "move-before", id: dragRuleId, beforeId });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setDragRuleId(null);
    }
  };

  return (
    <section className="truth-workspace rules-workspace">
      <div className="release-hero">
        <div>
          <div className="rules-title-row">
            <p className="eyebrow">Rules</p>
            <button type="button" className="rules-add-button" onClick={() => createRuleDraft()}>Add Rule</button>
          </div>
          <h2>Project Rules</h2>
          <p>{selectedCategoryLabel}: ranked architecture, development, design, release, and project-specific rules.</p>
        </div>
        <div className="release-state-badges">
          <Badge>{visibleRules.length} rules</Badge>
          <Badge tone={ruleCriticalityTone("critical")}>{visibleRules.filter((rule) => rule.criticality === "critical").length} critical</Badge>
        </div>
      </div>
      <div className="rules-list">
        {visibleRules.length ? visibleRules.map((rule) => (
          <button
            key={rule.id}
            type="button"
            draggable={!rule.protection.edit && !rule.protection.delete}
            className={`rule-row ${selectedRule?.id === rule.id ? "active" : ""} ${dragRuleId === rule.id ? "dragging" : ""}`}
            onClick={() => onSelectRule(rule.id)}
            onDragStart={() => setDragRuleId(rule.id)}
            onDragOver={(event) => {
              if (!rule.protection.edit && !rule.protection.delete) event.preventDefault();
            }}
            onDrop={(event) => {
              event.preventDefault();
              dropRuleBefore(rule.id);
            }}
            onDragEnd={() => setDragRuleId(null)}
          >
            <span className="rule-order">{rule.order}</span>
            <div className="rule-row-main">
              <div className="rule-row-title">
                <strong>{rule.title}</strong>
                <Badge tone={ruleCriticalityTone(rule.criticality)}>{rule.criticality}</Badge>
                <Badge>{rule.category}</Badge>
                <Badge>{ruleProtectionLabel(rule)}</Badge>
              </div>
              <p>{rule.summary}</p>
            </div>
          </button>
        )) : (
          <article className="rule-row passive">
            <span className="rule-order">0</span>
            <div className="rule-row-main">
              <div className="rule-row-title">
                <strong>No rules in this category</strong>
                <Badge>{selectedCategoryLabel}</Badge>
              </div>
              <p>Add a rule to this category, or select another category from the browse pane.</p>
            </div>
          </article>
        )}
      </div>
      {draft ? (
        <div className="rule-editor">
          <div className="rule-editor-head">
            <h3>{draftIsNew ? "Add Rule" : "Edit Rule"}</h3>
          </div>
          <div className="rule-editor-grid">
            <label>
              ID
              <input aria-label="ID" value={draft.id} disabled={!draftIsNew} onChange={(event) => setDraft({ ...draft, id: slugifyRuleId(event.target.value) })} />
            </label>
            <label>
              Title
              <input aria-label="Title" value={draft.title} disabled={draftEditProtected} onChange={(event) => setDraft({ ...draft, title: event.target.value })} />
            </label>
            <label>
              Criticality
              <select aria-label="Criticality" value={draft.criticality} disabled={draftEditProtected} onChange={(event) => setDraft({ ...draft, criticality: event.target.value as RuleItem["criticality"] })}>
                <option value="critical">Critical</option>
                <option value="high">High</option>
                <option value="medium">Medium</option>
                <option value="low">Low</option>
              </select>
            </label>
            <label>
              Category
              <input aria-label="Category" value={draft.category} disabled={draftEditProtected} onChange={(event) => setDraft({ ...draft, category: event.target.value })} />
            </label>
            <label>
              Source
              <select aria-label="Source" value={draft.source} disabled={draftEditProtected} onChange={(event) => setDraft({ ...draft, source: event.target.value as RuleItem["source"] })}>
                <option value="maintainer">Maintainer</option>
                <option value="agent">Agent</option>
              </select>
            </label>
            <label className="rule-protection-check">
              <input type="checkbox" checked={draft.protection.edit} disabled={draftEditProtected} onChange={(event) => setDraft({ ...draft, protection: { ...draft.protection, edit: event.target.checked } })} />
              Edit protected
            </label>
            <label className="rule-protection-check">
              <input type="checkbox" checked={draft.protection.delete} disabled={draftEditProtected} onChange={(event) => setDraft({ ...draft, protection: { ...draft.protection, delete: event.target.checked } })} />
              Delete protected
            </label>
          </div>
          <label className="rule-editor-summary">
            Summary
            <textarea aria-label="Summary" value={draft.summary} disabled={draftEditProtected} onChange={(event) => setDraft({ ...draft, summary: event.target.value })} />
          </label>
          {message ? <p className="rule-editor-message">{message}</p> : null}
          <div className="rule-editor-actions rule-editor-actions-footer">
            <button type="button" className="secondary-action" onClick={() => moveSelectedRule("up")} disabled={draftCannotMove || pendingAction}>Move up</button>
            <button type="button" className="secondary-action" onClick={() => moveSelectedRule("down")} disabled={draftCannotMove || pendingAction}>Move down</button>
            <button type="button" className="quiet-action" onClick={() => {
              setDraft(selectedRule);
              setMessage("");
              onEditingChange(false);
            }} disabled={pendingAction}>Cancel</button>
            <button type="button" className="danger-action" onClick={deleteSelectedRule} disabled={draftCannotDelete || pendingAction}>Delete</button>
            <button type="button" className="primary-action" onClick={saveDraft} disabled={draftEditProtected || pendingAction}>Save rule</button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

function RulesDetails({ rule }: { rule: RuleItem | null }) {
  if (!rule) {
    return (
      <div className="detail-content">
        <p className="eyebrow">Rules</p>
        <h2>No rule selected</h2>
      </div>
    );
  }

  return (
    <DetailShell eyebrow="Rule" title={rule.title} summary={rule.summary}>
      <div className="badge-row">
        <Badge tone={ruleCriticalityTone(rule.criticality)}>{rule.criticality}</Badge>
        <Badge>{rule.category}</Badge>
        <Badge>{rule.source}</Badge>
        <Badge>{ruleProtectionLabel(rule)}</Badge>
      </div>
      <OptionalFieldList title="Rationale" items={rule.rationale ? [rule.rationale] : []} />
      <OptionalFieldList title="Applies To" items={rule.appliesTo ?? []} />
      <FieldList title="Protection" items={[
        rule.protection.edit ? "Edit protected" : "Editable",
        rule.protection.delete ? "Delete protected" : "Delete allowed"
      ]} />
    </DetailShell>
  );
}

function ReleaseTruthWorkspace({
  releases,
  activeReleaseSummary,
  activeReleaseDetail,
  selection,
  onSelectCurrentRelease,
  onSelectReleaseItem,
  onSelectReleaseMilestone,
  planningMode,
  onPlanningModeChange,
  roadmapItems,
  onReleasePlanApproved,
  onReleasePlanningEditingChange
}: {
  releases?: ReleaseModel;
  activeReleaseSummary: ReleaseSummary | null;
  activeReleaseDetail: ReleaseDetail | null;
  selection: Selection | null;
  onSelectCurrentRelease: () => void;
  onSelectReleaseItem: (id: Id) => void;
  onSelectReleaseMilestone: (id: Id) => void;
  planningMode: boolean;
  onPlanningModeChange: (value: boolean) => void;
  roadmapItems?: RoadmapItem[];
  onReleasePlanApproved: () => Promise<void>;
  onReleasePlanningEditingChange: (editing: boolean) => void;
}) {
  const [releaseProjection, setReleaseProjection] = useState<"path" | "kanban">("path");

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
  const canEditRelease = activeReleaseSummary.status !== "completed";

  return (
    <section className="release-truth-workspace">
      <header className="release-hero">
        <div>
          <div className="release-hero-heading">
            <p className="eyebrow">Release Truth</p>
            {canEditRelease ? (
              <button type="button" className="button-reset release-edit-button" onClick={() => onPlanningModeChange(!planningMode)}>
                {planningMode ? "View truth" : "Edit plan"}
              </button>
            ) : null}
          </div>
          <h2>{activeReleaseSummary.name}</h2>
          <p>{activeReleaseSummary.summary}</p>
        </div>
        <div className="release-hero-meta">
          <div className="release-hero-status">
            <ReleaseStateBadges status={activeReleaseSummary.status} posture={activeReleaseSummary.posture} />
          </div>
          <span className="release-hero-updated">Updated {formatReleaseDate(activeReleaseSummary.lastUpdated)}</span>
          {releases.index.currentReleaseId !== activeReleaseSummary.id ? (
            <div className="release-hero-actions">
              <button type="button" className="button-reset release-current-button" onClick={onSelectCurrentRelease}>
                Current release
              </button>
            </div>
          ) : null}
        </div>
      </header>

      {planningMode && canEditRelease ? (
        <ReleasePlanningPanel
          releaseIndex={releases.index}
          roadmapItems={roadmapItems ?? []}
          activeReleaseSummary={activeReleaseSummary}
          activeReleaseDetail={activeReleaseDetail}
          onApproved={onReleasePlanApproved}
          onEditingChange={onReleasePlanningEditingChange}
        />
      ) : (
        <>
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
          <div className="release-section-head">
            <h3>{releaseProjection === "path" ? "Release Path" : "Kanban"}</h3>
            <div className="release-view-toggle" aria-label="Release Truth projection">
              <button type="button" className={releaseProjection === "path" ? "active" : ""} onClick={() => setReleaseProjection("path")}>
                Path
              </button>
              <button type="button" className={releaseProjection === "kanban" ? "active" : ""} onClick={() => setReleaseProjection("kanban")}>
                Kanban
              </button>
            </div>
          </div>
          {activeReleaseDetail ? (
            releaseProjection === "path" ? (
              <ReleasePath
                detail={activeReleaseDetail}
                selection={selection}
                onSelectItem={onSelectReleaseItem}
                onSelectMilestone={onSelectReleaseMilestone}
              />
            ) : (
              <ReleaseKanban
                detail={activeReleaseDetail}
                selection={selection}
                onSelectItem={onSelectReleaseItem}
              />
            )
          ) : (
            <p className="muted">Release detail is loading.</p>
          )}
        </section>

        <section className="release-section release-section-wide release-history-section">
          <h3>History</h3>
          <ReleaseTrendChart releases={releases.index.releases} activeReleaseId={activeReleaseSummary.id} />
        </section>
      </div>
        </>
      )}
    </section>
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
    const state = releaseLineState(item.status, blockers.length > 0);
    const showScopeBadge = scope.toLowerCase().replaceAll(" ", "-") !== item.status;
    const decisionItems = [item.rationale, item.decisionSource].filter(Boolean) as string[];
    const blockerItems = blockers.map((blocker) => `${blocker.title}: ${blocker.summary}`);
    const nextActionItems = blockers.map((blocker) => blocker.nextAction);
    const dependencyItems = dependencies.map((dependency) => dependency.summary);
    const evidenceItems = evidence.map((item) => item.href ? `${item.label} (${item.href})` : item.label);
    const sections = [
      "Workstream",
      ...(decisionItems.length ? ["Decision"] : []),
      ...(blockerItems.length ? ["Blockers"] : []),
      ...(nextActionItems.length ? ["Next Actions"] : []),
      ...(dependencyItems.length ? ["Dependencies"] : []),
      ...(evidenceItems.length ? ["Evidence"] : [])
    ];
    return (
      <DetailShell eyebrow="Release item" title={item.title} summary={item.summary} sections={sections}>
        <div className="badge-row">
          <Badge tone={releaseBadgeTone(state === "Blocked" ? "blocked" : item.status)}>{state}</Badge>
          {showScopeBadge ? <Badge>{scope}</Badge> : null}
          <Badge>{item.kind}</Badge>
          {item.priority ? <Badge tone={releaseBadgeTone(item.priority)}>{item.priority} priority</Badge> : null}
          {item.owner ? <Badge>{item.owner}</Badge> : null}
        </div>
        <FieldList title="Workstream" items={[workstream?.name ?? "Unassigned"]} />
        <OptionalFieldList title="Decision" items={decisionItems} />
        <OptionalFieldList title="Blockers" items={blockerItems} />
        <OptionalFieldList title="Next Actions" items={nextActionItems} />
        <OptionalFieldList title="Dependencies" items={dependencyItems} />
        <OptionalFieldList title="Evidence" items={evidenceItems} />
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
    const blockers = activeReleaseBlockersForItem(item, blockersByItemId.get(item.id) ?? []);
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
      blockers: milestoneItems.filter((item) => item.status === "blocked" || activeReleaseBlockersForItem(item, blockersByItemId.get(item.id) ?? []).length > 0)
    };
  }

  return null;
}

function shortDiagnosticLine(text: string) {
  const line = text.split(/\r?\n/).map((entry) => entry.trim()).find(Boolean) ?? "No diagnostic output.";
  return line.length > 140 ? `${line.slice(0, 137)}...` : line;
}

function recoveryDetailsText(error: string, result: RecoveryResult | null) {
  if (!result) return error;
  const primary = result.error
    ?? result.output
    ?? result.validation?.output
    ?? result.status?.validation?.output
    ?? error;
  const responseJson = JSON.stringify(result, null, 2);
  return `${primary}\n\nRecovery response:\n${responseJson}`;
}

function RecoveryShell({
  error,
  busy,
  result,
  onAction
}: {
  error: string;
  busy: string | null;
  result: RecoveryResult | null;
  onAction: (action: "reload" | "status" | "doctor-dry-run" | "doctor-apply" | "sync-repair") => void;
}) {
  const repairs = result?.repairs ?? result?.status?.doctorRepairs ?? [];
  const diagnostic = result?.error
    ?? result?.output
    ?? result?.validation?.output
    ?? result?.status?.validation?.output
    ?? error;
  const details = recoveryDetailsText(error, result);
  const shortDiagnostic = shortDiagnosticLine(diagnostic);
  const resultLabel = result
    ? `${(result.mode ?? "status").replaceAll("-", " ").toUpperCase()}: ${result.ok ? "COMPLETE" : "NEEDS ATTENTION"}`
    : null;
  const resultSummary = result?.error && result.mode === "status"
    ? `Status check found invalid data: ${shortDiagnostic}`
    : result?.error
      ? `Action failed: ${shortDiagnostic}`
      : result?.validation?.ok === false || result?.status?.validation?.ok === false || result?.ok === false
    ? `Validation is still failing: ${shortDiagnostic}`
    : result?.reload
      ? "Validation passed. Reloading data."
      : result
        ? "Recovery action completed."
        : null;

  return (
    <div className="app recovery-app">
      <header className="topbar">
        <div>
          <p className="eyebrow">Architext / {__ARCHITEXT_VERSION__}</p>
          <div className="project-title-line">
            <h1>Recovery</h1>
            <p>Target data could not produce a valid architecture model.</p>
          </div>
        </div>
        <div className="topbar-actions">
          <button type="button" onClick={() => onAction("reload")} disabled={Boolean(busy)}>
            RELOAD
          </button>
        </div>
      </header>
      <main className="recovery-shell">
        <section className="recovery-panel" aria-live="polite" aria-busy={Boolean(busy)}>
          <p className="eyebrow">Data Health</p>
          <h2>Architext is running in recovery mode</h2>
          <p>
            The served viewer is available, but the current data failed loading
            or validation. Use the constrained repair actions below; they reuse
            Architext doctor and sync behavior.
          </p>
          <div className="recovery-actions">
            <button type="button" onClick={() => onAction("status")} disabled={Boolean(busy)}>CHECK STATUS</button>
            <button type="button" onClick={() => onAction("doctor-dry-run")} disabled={Boolean(busy)}>DOCTOR DRY RUN</button>
            <button type="button" onClick={() => onAction("doctor-apply")} disabled={Boolean(busy)}>APPLY DOCTOR</button>
            <button type="button" onClick={() => onAction("sync-repair")} disabled={Boolean(busy)}>SYNC REPAIR</button>
          </div>
          {resultLabel ? (
            <div className="recovery-result">
              <strong>{resultLabel}</strong>
              {resultSummary ? <span>{resultSummary}</span> : null}
            </div>
          ) : null}
          {repairs.length ? (
            <div className="recovery-repairs">
              <strong>Repair candidates</strong>
              <ul>
                {repairs.map((repair, index) => (
                  <li key={`${repair.category ?? "repair"}-${index}`}>{repair.summary ?? repair.file ?? "Repair"}</li>
                ))}
              </ul>
            </div>
          ) : null}
          <details className="recovery-details">
            <summary>DETAILS</summary>
            <pre>{details}</pre>
          </details>
        </section>
      </main>
    </div>
  );
}

function App() {
  const [query, setQuery] = useState("");
  const [activeMode, setActiveMode] = useState<Mode>("flows");
  const [activeViewId, setActiveViewId] = useState<Id>("");
  const [activeFlowId, setActiveFlowId] = useState<Id>("");
  const [activeReleaseId, setActiveReleaseId] = useState<Id>("");
  const [releaseDetailsById, setReleaseDetailsById] = useState<Map<Id, ReleaseDetail>>(new Map());
  const [selection, setSelection] = useState<Selection | null>(null);
  const [releasePlanningMode, setReleasePlanningMode] = useState(false);
  const [releasePlanningDirty, setReleasePlanningDirty] = useState(false);
  const [rulesEditorDirty, setRulesEditorDirty] = useState(false);
  const [selectedRuleCategory, setSelectedRuleCategory] = useState("all");
  const [newRuleDraftRequest, setNewRuleDraftRequest] = useState<{ id: number; category: string } | null>(null);
  const [riskFilter, setRiskFilter] = useState("all");
  // User-configurable diagram parameters. configPayload holds the server response
  // { diagram, fields, sections }; configDraft holds unsaved live-preview edits.
  // The effective config (draft over saved) feeds both the diagram and the
  // settings panel. Null until loaded / on static builds -> hardcoded defaults.
  const [configPayload, setConfigPayload] = useState<{ diagram: DiagramConfig; fields: DiagramFieldsSpec; sections: DiagramSectionLabels } | null>(null);
  const [configDraft, setConfigDraft] = useState<DiagramConfig | null>(null);
  const [configPanelOpen, setConfigPanelOpen] = useState(false);
  const [repoTree, setRepoTree] = useState<{ files: Array<{ path: string; size: number | null; mtime: number | null }>; source?: string } | null>(null);
  const [repoTreeLens, setRepoTreeLens] = useState<"c4" | "flow">("c4");
  const [blastQuery, setBlastQuery] = useState("");
  const [blastFocusId, setBlastFocusId] = useState<Id | null>(null);
  const [configBusy, setConfigBusy] = useState(false);
  const [configMessage, setConfigMessage] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    fetchDiagramConfig().then((payload) => {
      if (!cancelled) setConfigPayload(payload);
    });
    return () => { cancelled = true; };
  }, []);
  // Lazily fetch the repo file list the first time Repo Tree or Blast Radius
  // (which maps files to owning components) is opened.
  useEffect(() => {
    if ((activeMode !== "repo-tree" && activeMode !== "blast-radius") || repoTree) return;
    let cancelled = false;
    fetchRepoTree().then((result) => {
      if (!cancelled) setRepoTree(result ?? { files: [] });
    });
    return () => { cancelled = true; };
  }, [activeMode, repoTree]);
  const savedConfig = configPayload?.diagram ?? null;
  const effectiveConfig = configDraft ?? savedConfig;

  const updateConfigField = useCallback((section: string, field: string, next: number) => {
    setConfigDraft((current) => {
      const base = (current ?? savedConfig ?? {}) as Record<string, Record<string, number>>;
      return { ...base, [section]: { ...base[section], [field]: next } } as DiagramConfig;
    });
  }, [savedConfig]);

  const resetConfigToDefaults = useCallback(() => {
    const fields = configPayload?.fields;
    if (!fields) return;
    const defaults: Record<string, Record<string, number>> = {};
    for (const [section, sectionFields] of Object.entries(fields)) {
      defaults[section] = Object.fromEntries(
        Object.entries(sectionFields).map(([field, spec]) => [field, spec.default])
      );
    }
    setConfigDraft(defaults as DiagramConfig);
  }, [configPayload]);

  const revertConfig = useCallback(() => setConfigDraft(null), []);

  const saveConfig = useCallback(async (scope: "project" | "user") => {
    if (!effectiveConfig) return;
    setConfigBusy(true);
    setConfigMessage(null);
    try {
      const result = await postDiagramConfig(mutationFetch, { scope, diagram: effectiveConfig });
      setConfigPayload((prev) => (prev ? { ...prev, diagram: result.diagram as DiagramConfig } : prev));
      setConfigDraft(null);
      setConfigMessage(scope === "user" ? "Saved to ~/.architext/config.json." : "Saved to docs/architext/config.json.");
    } catch (error) {
      setConfigMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setConfigBusy(false);
    }
  }, [effectiveConfig]);
  const {
    debugRouting,
    diagramTransform,
    diagramViewportRef,
    fitDisplayedDiagram,
    navCollapsed,
    rightCollapsed,
    stepsCollapsed,
    routingStyle,
    setDiagramTransform,
    setNavCollapsed,
    setRightCollapsed,
    setStepsCollapsed,
    setRoutingStyle
  } = useDiagramViewport({
    localStorage,
    locationSearch: window.location.search,
    zoomConfig: effectiveConfig?.zoom ?? null
  });
  const editorStates = useMemo(() => [
    { id: "release-planning", label: "Release Planning", dirty: releasePlanningDirty },
    { id: "rules", label: "Rules editor", dirty: rulesEditorDirty }
  ], [releasePlanningDirty, rulesEditorDirty]);
  const { confirmEditorNavigation } = useUnsavedEditorGuard(editorStates);

  const onModelLoaded = useCallback((loaded: Model, resetSelection: boolean) => {
    const requestedMode = modeForHash(window.location.hash) as Mode | null;
    if (resetSelection) {
      const fallback = loaded.views.find((view) => view.id === loaded.manifest.defaultViewId) ?? loaded.views[0];
      const nextMode = requestedMode ?? modeForView(fallback);
      const nextView = defaultViewForMode(nextMode, loaded.views, fallback);
      setActiveViewId(nextView.id);
      setActiveMode(nextMode);
      setActiveFlowId(loaded.flows[0]?.id ?? "");
      setSelection(nextMode === "release-truth" || nextMode === "rules" ? null : { kind: "flow", id: loaded.flows[0]?.id ?? "" });
    } else {
      setActiveViewId((current) => loaded.views.some((view) => view.id === current) ? current : loaded.manifest.defaultViewId);
      setActiveFlowId((current) => loaded.flows.some((flow) => flow.id === current) ? current : loaded.flows[0]?.id ?? "");
    }
    setActiveReleaseId(loaded.releases?.index.currentReleaseId ?? "");
    setReleaseDetailsById(new Map((loaded.releases?.details ?? []).map((detail) => [detail.id, detail])));
  }, []);

  const {
    applyLoadedModel,
    dataIssue,
    dataNotice,
    error,
    model,
    recoveryBusy,
    recoveryResult,
    reloadArchitectureData,
    reloadInvalidDataNow,
    requestRecoveryAction,
    setDataIssue,
    setDataNotice,
    setModel
  } = useArchitextModel({
    releasePlanningDirty,
    rulesEditorDirty,
    onModelLoaded
  });

  const reloadReleasePlanningModel = async () => {
    const loaded = await loadArchitectureModel();
    const selectedReleaseId = selectedReleaseIdForReload(activeReleaseId, loaded.releases);
    const releaseDetails = await releaseDetailsForSelectedRelease(fetch, loaded.releases, selectedReleaseId);
    setModel(loaded);
    setActiveReleaseId((current) => {
      const releases: ReleaseSummary[] = loaded.releases?.index.releases ?? [];
      if (current && releases.some((release) => release.id === current)) return current;
      return loaded.releases?.index.currentReleaseId ?? "";
    });
    setReleaseDetailsById(releaseDetails);
    setSelection(null);
  };

  useEffect(() => {
    const onHashChange = () => {
      const requestedMode = modeForHash(window.location.hash) as Mode | null;
      if (!requestedMode || !model) return;
      switchMode(requestedMode, { restoreHashOnCancel: true });
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [activeMode, confirmEditorNavigation, model]);

  if (error) {
    return <RecoveryShell error={error} busy={recoveryBusy} result={recoveryResult} onAction={requestRecoveryAction} />;
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
  const activeFlowStepDisplayIndexes = flowStepDisplayIndexes(activeFlow.steps);
  const fallbackView = model.views[0];
  const selectedView = viewsById.get(activeViewId);
  const modeView = viewBelongsToMode(selectedView, activeMode)
    ? selectedView
    : defaultViewForMode(activeMode, model.views, fallbackView);
  const activeView = defaultViewForFlow(activeMode, modeView, model.views, activeFlow, fallbackView) as View;
  const isC4View = activeMode === "c4";
  const isSequenceView = activeMode === "sequence";
  const isReleaseTruthView = activeMode === "release-truth";
  const isRulesView = activeMode === "rules";
  const isRepoTreeView = activeMode === "repo-tree";
  const isBlastView = activeMode === "blast-radius";
  // Cheap derivations (tens of nodes/files) — plain consts, not hooks, so they
  // sit after the model guard without breaking hook order.
  const blastFiles = repoTree?.files ?? [];
  const blastSearchResults = isBlastView ? searchRepository(model, blastFiles, blastQuery) : { components: [], files: [] };
  const blastRadius = isBlastView && blastFocusId ? blastRadiusForNode(model, blastFiles, blastFocusId) : null;
  const showOrderedFlow = modeShowsOrderedFlow(activeMode);
  const showStructuralConnections = modeUsesStructuralRelationships(activeMode);
  const showStepSummary = showOrderedFlow;
  const flowNodeIds = new Set(activeFlow.steps.flatMap((step) => [step.from, step.to]));
  const selectedNodeId = selection?.kind === "node" ? selection.id : null;
  const selectedStepId = selectedStepIdForSelection(selection);
  const selectedFlowId = selectedFlowIdForSelection(selection);
  const selectedActiveStepId = activeFlow.steps.some((step) => isSelectedStep(selection, activeFlow.id, step.id)) ? selectedStepId : null;
  const selectedActiveStepDisplayIndex = selectedActiveStepId ? activeFlowStepDisplayIndexes.get(selectedActiveStepId) ?? null : null;
  const selectedFlowForStep = selectedFlowId ? flowsById.get(selectedFlowId) : null;
  const selectedStep = selectedStepId
    ? selectedFlowForStep?.steps.find((step) => step.id === selectedStepId) ?? null
    : null;
  const activeReleaseSummary = model.releases?.index.releases.find((release) => release.id === activeReleaseId)
    ?? model.releases?.index.releases.find((release) => release.id === model.releases?.index.currentReleaseId)
    ?? null;
  const activeReleaseDetail = activeReleaseSummary ? releaseDetailsById.get(activeReleaseSummary.id) ?? null : null;
  const selectedRule = selection?.kind === "rule"
    ? model.rules?.find((rule) => rule.id === selection.id) ?? null
    : null;

  const visibleFlows = activeMode === "flows" ? compatibleFlowsForView(model.flows, activeView) as Flow[] : model.flows;
  const filteredFlows = visibleFlows.filter((flow) => {
    const text = [flow.name, flow.summary, flow.status, flow.trigger, ...flow.knownGaps].join(" ").toLowerCase();
    return text.includes(query.toLowerCase());
  });

  const clearEditorDirtyState = () => {
    setReleasePlanningDirty(false);
    setRulesEditorDirty(false);
  };

  const confirmEditorNavigationOrStay = () => {
    if (confirmEditorNavigation()) {
      clearEditorDirtyState();
      return true;
    }
    return false;
  };

  const guardedSelect = (select: () => void, onCancel?: () => void) => {
    if (!confirmEditorNavigationOrStay()) {
      onCancel?.();
      return false;
    }
    select();
    return true;
  };

  const switchMode = (mode: Mode, options: { restoreHashOnCancel?: boolean } = {}) => {
    guardedSelect(() => {
      const nextView = defaultViewForMode(mode, model.views, fallbackView);
      window.history.replaceState(null, "", hashForMode(mode));
      setActiveMode(mode);
      setActiveViewId(nextView.id);
      setSelection(null);
      if (mode === "release-truth" && !activeReleaseId && model.releases?.index.currentReleaseId) {
        setActiveReleaseId(model.releases.index.currentReleaseId);
      }
    }, options.restoreHashOnCancel ? () => window.history.replaceState(null, "", hashForMode(activeMode)) : undefined);
  };

  const selectRelease = async (releaseId: Id) => {
    if (!guardedSelect(() => {
      setActiveReleaseId(releaseId);
      setSelection(null);
    })) return;
    if (!model.releases || releaseDetailsById.has(releaseId)) return;
    const detail = await loadReleaseDetail(fetch, model.releases, releaseId);
    setReleaseDetailsById((current) => new Map(current).set(detail.id, detail));
  };

  const selectCurrentRelease = () => {
    const currentReleaseId = model.releases?.index.currentReleaseId;
    if (currentReleaseId) void selectRelease(currentReleaseId);
  };

  const selectView = (viewId: Id) => {
    guardedSelect(() => {
      const view = viewsById.get(viewId);
      if (!view) return;
      const nextMode = modeForView(view);
      window.history.replaceState(null, "", hashForMode(nextMode));
      setActiveMode(nextMode);
      setActiveViewId(viewId);
      if (nextMode === "flows") {
        const nextFlow = defaultFlowForView(view, activeFlow, model.flows, model.flows[0]) as Flow;
        if (nextFlow?.id && nextFlow.id !== activeFlow.id) {
          setActiveFlowId(nextFlow.id);
          setSelection({ kind: "flow", id: nextFlow.id });
          return;
        }
      }
      if (nextMode === "c4") {
        const firstNodeId = view.lanes.flatMap((lane) => lane.nodeIds)[0];
        if (firstNodeId) setSelection({ kind: "node", id: firstNodeId });
        return;
      }
      setSelection(null);
    });
  };

  const selectC4Node = (nodeId: Id) => {
    guardedSelect(() => {
      const childView = childC4ViewForNode(model.views, activeView, nodeId) as View | null;
      if (childView) {
        setActiveViewId(childView.id);
        const firstNodeId = childView.lanes.flatMap((lane) => lane.nodeIds)[0] ?? nodeId;
        setSelection({ kind: "node", id: firstNodeId });
        return;
      }
      const node = nodesById.get(nodeId);
      const reason = c4DrilldownUnavailableReason(activeView, node);
      if (reason) setDataNotice(reason);
      setSelection({ kind: "node", id: nodeId });
    });
  };

  const selectNode = (id: Id) => {
    guardedSelect(() => {
      setSelection({ kind: "node", id });
    });
  };

  const selectFlow = (flowId: Id) => {
    guardedSelect(() => {
      const nextFlow = flowsById.get(flowId);
      if (nextFlow && activeMode === "flows") {
        const nextView = defaultViewForFlow(activeMode, activeView, model.views, nextFlow, fallbackView) as View;
        if (nextView?.id && nextView.id !== activeView.id) setActiveViewId(nextView.id);
      }
      setActiveFlowId(flowId);
      setSelection({ kind: "flow", id: flowId });
    });
  };

  const saveNote = async (note: ElementNote) => {
    await postNotesAction(mutationFetch, { action: "update", note });
    await reloadArchitectureData();
  };
  const deleteNoteById = async (id: Id) => {
    await postNotesAction(mutationFetch, { action: "delete", id });
    await reloadArchitectureData();
  };

  const selectRule = (id: Id) => {
    guardedSelect(() => {
      setSelection({ kind: "rule", id });
      setRightCollapsed(false);
    });
  };

  const selectRuleCategory = (category: string) => {
    guardedSelect(() => {
      setSelectedRuleCategory(category);
      setSelection(null);
    });
  };

  const addRuleCategory = () => {
    guardedSelect(() => {
      const category = nextRuleCategoryName(model.rules ?? []);
      setSelectedRuleCategory(category);
      setSelection(null);
      setNewRuleDraftRequest((current) => ({ id: (current?.id ?? 0) + 1, category }));
    });
  };

  const selectReleaseItem = (id: Id) => {
    guardedSelect(() => {
      setSelection({ kind: "release-item", itemId: id });
      setRightCollapsed(false);
    });
  };

  const selectReleaseMilestone = (id: Id) => {
    guardedSelect(() => {
      setSelection({ kind: "release-milestone", milestoneId: id });
      setRightCollapsed(false);
    });
  };

  const setPlanningMode = (nextPlanningMode: boolean) => {
    if (!nextPlanningMode) {
      guardedSelect(() => setReleasePlanningMode(nextPlanningMode));
      return;
    }
    setReleasePlanningMode(nextPlanningMode);
  };

  const saveSelectedRule = (id: Id) => {
    setSelection({ kind: "rule", id });
    setRightCollapsed(false);
  };

  const selectDiagramNode = (id: Id) => {
    guardedSelect(() => {
      setSelection({ kind: "node", id });
    });
  };

  const selectRelationship = (relationship: Relationship) => {
    guardedSelect(() => {
      if (
        relationship.stepId &&
        selection?.kind === "step" &&
        selection.flowId === relationship.flowId &&
        selection.stepId === relationship.stepId
      ) {
        setSelection({ kind: "flow", id: relationship.flowId ?? activeFlow.id });
        return;
      }
      setSelection({
        kind: "relationship",
        from: relationship.from,
        to: relationship.to,
        label: relationship.label,
        relationshipType: relationship.relationshipType,
        stepId: relationship.stepId,
        flowId: relationship.flowId
      });
    });
  };

  const selectActiveStep = (stepId: Id) => {
    guardedSelect(() => {
      setSelection((current) => (
        current?.kind === "step" && current.flowId === activeFlow.id && current.stepId === stepId
          ? { kind: "flow", id: activeFlow.id }
          : { kind: "step", flowId: activeFlow.id, stepId }
      ));
    });
  };

  const exportActiveViewPdf = () => {
    const result = requestPdfExport({
      print: window.print,
      requestAnimationFrame: window.requestAnimationFrame.bind(window)
    });
    setDataNotice(result.message);
  };

  const toggleDiagramFocus = () => {
    const nextFocused = !diagramTransform.focused;
    setDiagramTransform((value) => ({ ...value, focused: nextFocused }));
    if (nextFocused) window.requestAnimationFrame(fitDisplayedDiagram);
  };

  return (
    <DiagramConfigContext.Provider value={effectiveConfig}>
    <div className={`app ${navCollapsed ? "left-collapsed" : ""} ${rightCollapsed ? "right-collapsed" : ""} ${diagramTransform.focused ? "diagram-focused" : ""}`}>
      <header className="topbar">
        <div>
          <p className="eyebrow">Architext / {__ARCHITEXT_VERSION__} · Schema / {model.manifest.schemaVersion}</p>
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
          <button
            type="button"
            className="topbar-config-button"
            onClick={() => setConfigPanelOpen(true)}
            title="Configure diagram spacing, layout, and zoom"
            aria-label="Open diagram configuration"
          >
            <span aria-hidden="true">⚙</span> Config
          </button>
        </div>
      </header>
      {dataNotice && !dataIssue ? (
        <div className="data-refresh-notice" role="status">
          <span>{dataNotice}</span>
        </div>
      ) : null}
      {dataIssue ? (
        <div className="data-issue-backdrop" role="presentation">
          <section className="data-issue-dialog" role="dialog" aria-modal="true" aria-labelledby="data-issue-title" aria-describedby="data-issue-description">
            <p className="eyebrow">Data Health</p>
            <h2 id="data-issue-title">Architext data is invalid</h2>
            <p id="data-issue-description">Wait keeps checking in the background and refreshes this viewer when validation passes. Now tries to reload immediately.</p>
            <details className="data-issue-details">
              <summary>DETAILS</summary>
              <pre>{dataIssue.message}</pre>
            </details>
            <div className="data-issue-actions">
              <button type="button" onClick={() => setDataIssue(null)}>WAIT</button>
              <button type="button" className="primary-action" onClick={reloadInvalidDataNow}>REFRESH NOW</button>
            </div>
          </section>
        </div>
      ) : null}

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

      <aside className="left-nav">
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
            rules={model.rules}
            activeReleaseId={activeReleaseSummary?.id ?? ""}
            activeRuleCategory={selectedRuleCategory}
            riskFilter={riskFilter}
            onRiskFilterChange={setRiskFilter}
            repoTreeLens={repoTreeLens}
            onRepoTreeLensChange={setRepoTreeLens}
            blastQuery={blastQuery}
            onBlastQueryChange={setBlastQuery}
            blastSearchResults={blastSearchResults}
            blastFocusId={blastFocusId}
            onFocusBlastNode={(id: Id) => { setBlastFocusId(id); selectNode(id); }}
            onSelectFlow={selectFlow}
            onSelectView={selectView}
            onSelectNode={selectNode}
            onSelectRelease={selectRelease}
            onSelectRule={selectRule}
            onSelectRuleCategory={selectRuleCategory}
            onAddRuleCategory={addRuleCategory}
          />
        )}
      </aside>

      <main className="diagram-area">
        {isBlastView ? (
          <BlastRadiusWorkspace
            radius={blastRadius}
            hasQuery={blastQuery.trim().length > 0}
            onFocusNode={(id) => { setBlastFocusId(id); selectNode(id); }}
            onSelectFlow={(id) => { setActiveMode("flows"); selectFlow(id); }}
            onSelectView={selectView}
          />
        ) : isRepoTreeView ? (
          <RepoTreeWorkspace
            files={repoTree?.files ?? []}
            source={repoTree?.source}
            nodes={model.nodes}
            flows={model.flows}
            lens={repoTreeLens}
            onSelectNode={selectNode}
          />
        ) : isRulesView ? (
          <RulesWorkspace
            rules={model.rules ?? []}
            selectedRule={selectedRule}
            selectedCategory={selectedRuleCategory}
            newRuleDraftRequest={newRuleDraftRequest}
            onSelectRule={selectRule}
            onRuleSaved={saveSelectedRule}
            onRulesChanged={reloadArchitectureData}
            onEditingChange={setRulesEditorDirty}
          />
        ) : isReleaseTruthView ? (
          <ReleaseTruthWorkspace
            releases={model.releases}
            activeReleaseSummary={activeReleaseSummary}
            activeReleaseDetail={activeReleaseDetail}
            selection={selection}
            onSelectCurrentRelease={selectCurrentRelease}
            planningMode={releasePlanningMode}
            onPlanningModeChange={setPlanningMode}
            roadmapItems={model.roadmap}
            onReleasePlanApproved={reloadReleasePlanningModel}
            onReleasePlanningEditingChange={setReleasePlanningDirty}
            onSelectReleaseItem={selectReleaseItem}
            onSelectReleaseMilestone={selectReleaseMilestone}
          />
        ) : (
          <>
            <section className="diagram-header">
              <div className="diagram-title-line">
                <h2 title={isSequenceView ? activeFlow.name : activeView.name}>{isSequenceView ? activeFlow.name : activeView.name}</h2>
                <p title={isSequenceView ? activeFlow.summary : activeView.summary}>{isSequenceView ? activeFlow.summary : activeView.summary}</p>
              </div>
              <DiagramControls
                transform={diagramTransform}
                routingStyle={routingStyle}
                onRoutingStyleChange={setRoutingStyle}
                onZoomIn={() => setDiagramTransform((value) => ({ ...value, zoom: Math.min(1.6, Number((value.zoom + 0.1).toFixed(2))) }))}
                onZoomOut={() => setDiagramTransform((value) => ({ ...value, zoom: Math.max(0.7, Number((value.zoom - 0.1).toFixed(2))) }))}
                onFit={fitDisplayedDiagram}
                onReset={() => setDiagramTransform((value) => ({ ...value, zoom: 1 }))}
                onToggleFocus={toggleDiagramFocus}
                onExportPdf={exportActiveViewPdf}
              />
              <details className="legend">
                <summary>Legend</summary>
                <div>
                  {(["actor", "software-system", "client", "service", "worker", "queue", "data-store", "external-service"] as NodeType[]).map((type) => (
                    <span key={type}><DiagramIcon icon={iconForNodeType(type)} className={`legend-icon ${type}`} />{type}</span>
                  ))}
                  <span><DiagramIcon icon="start" className="legend-icon start" />start</span>
                  <span><DiagramIcon icon="decision" className="legend-icon decision" />decision</span>
                  <span><DiagramIcon icon="stop" className="legend-icon stop" />stop</span>
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
                  onSelectStep={selectActiveStep}
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
                  onSelectNode={selectC4Node}
                  onSelectRelationship={selectRelationship}
                />
              ) : (
                <SystemMap
                  view={activeView}
                  nodesById={nodesById}
                  activeFlow={showOrderedFlow ? activeFlow : null}
                  showStructuralConnections={showStructuralConnections}
                  selectedStepId={selectedActiveStepId}
                  selectedStepDisplayIndex={selectedActiveStepDisplayIndex}
                  selectedRelationship={selection?.kind === "relationship" ? selection : null}
                  selectedNodeId={selectedNodeId}
                  transform={diagramTransform}
                  routingStyle={routingStyle}
                  debugRouting={debugRouting}
                  onSelectNode={selectDiagramNode}
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
                  {activeFlow.steps.map((step, index) => ({ step, index })).filter(({ step, index }) => !isDecisionBranchSupportStep(activeFlow.steps, step, index)).map(({ step, index }) => (
                    <button
                      key={step.id}
                      type="button"
                      className={`step-card ${(activeFlowStepDisplayIndexes.get(step.id) ?? index + 1) === selectedActiveStepDisplayIndex ? "active" : ""}`}
                      onClick={() => {
                        selectActiveStep(step.id);
                      }}
                    >
                      <span className="step-kind-icon">
                        <DiagramIcon icon={iconForStep(step, index, activeFlow.steps.length)} className="step-icon" />
                      </span>
                      <span className="step-number">
                        <span>{activeFlowStepDisplayIndexes.get(step.id) ?? index + 1}</span>
                      </span>
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
        {rightCollapsed ? (
          <button type="button" className="panel-rail" onClick={() => setRightCollapsed(false)}>
            Details
          </button>
        ) : isRulesView ? (
          <RulesDetails rule={selectedRule} />
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
            notes={model.notes ?? []}
            onSaveNote={saveNote}
            onDeleteNote={deleteNoteById}
            onSelectNode={selectNode}
            onSelectFlow={selectFlow}
          />
        )}
      </aside>
      {configPanelOpen ? (
        <DiagramConfigPanel
          fields={configPayload?.fields ?? DIAGRAM_FIELD_SPEC}
          sections={configPayload?.sections ?? DIAGRAM_SECTION_LABELS}
          value={effectiveConfig ?? {}}
          saved={savedConfig ?? {}}
          busy={configBusy}
          message={configMessage}
          onChange={updateConfigField}
          onSave={saveConfig}
          onRevert={revertConfig}
          onResetDefaults={resetConfigToDefaults}
          onClose={() => setConfigPanelOpen(false)}
        />
      ) : null}
    </div>
    </DiagramConfigContext.Provider>
  );
}

// Compact node-type label for the search result list — long C4 types clip in
// the narrow nav, so collapse them to short tokens.
const SHORT_NODE_KIND: Record<string, string> = {
  "software-system": "system",
  "external-service": "external",
  "data-store": "store",
  "deployment-unit": "deploy",
  "trust-boundary": "trust"
};
function shortNodeKind(type: string): string {
  return SHORT_NODE_KIND[type] ?? type;
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
  rules,
  activeReleaseId,
  activeRuleCategory,
  riskFilter,
  onRiskFilterChange,
  repoTreeLens,
  onRepoTreeLensChange,
  blastQuery,
  onBlastQueryChange,
  blastSearchResults,
  blastFocusId,
  onFocusBlastNode,
  onSelectFlow,
  onSelectView,
  onSelectNode,
  onSelectRelease,
  onSelectRule,
  onSelectRuleCategory,
  onAddRuleCategory
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
  rules?: RuleItem[];
  activeReleaseId: Id;
  activeRuleCategory: string;
  riskFilter: string;
  onRiskFilterChange: (value: string) => void;
  repoTreeLens: "c4" | "flow";
  onRepoTreeLensChange: (lens: "c4" | "flow") => void;
  blastQuery: string;
  onBlastQueryChange: (value: string) => void;
  blastSearchResults: { components: Array<{ id: Id; name: string; type: string }>; files: Array<{ path: string; ownerId: Id | null; ownerName: string | null }> };
  blastFocusId: Id | null;
  onFocusBlastNode: (id: Id) => void;
  onSelectFlow: (id: Id) => void;
  onSelectView: (id: Id) => void;
  onSelectNode: (id: Id) => void;
  onSelectRelease: (id: Id) => void;
  onSelectRule: (id: Id) => void;
  onSelectRuleCategory: (category: string) => void;
  onAddRuleCategory: () => void;
}) {
  if (mode === "blast-radius") {
    const { components, files: fileHits } = blastSearchResults;
    const trimmed = blastQuery.trim();
    return (
      <>
        <div className="panel-head">
          <h2>Blast Radius</h2>
          <p>Search to see what an element reaches.</p>
        </div>
        <div className="repo-nav-section">
          <div className="blast-search-wrap">
            <DiagramIcon icon="search" className="blast-search-icon" />
            <input
              type="search"
              className="blast-search"
              placeholder="Search components, files…"
              value={blastQuery}
              onChange={(event) => onBlastQueryChange(event.target.value)}
              autoFocus
            />
          </div>
        </div>
        {trimmed ? (
          <>
            <div className="repo-nav-section">
              <h3 className="repo-nav-title">Components<span className="blast-count">{components.length}</span></h3>
              {components.length ? (
                <ul className="blast-results">
                  {components.map((c) => (
                    <li key={c.id}>
                      <button type="button" className={`blast-result${c.id === blastFocusId ? " active" : ""}`} onClick={() => onFocusBlastNode(c.id)}>
                        <span className="blast-result-name">{c.name}</span>
                        <span className="blast-result-kind">{shortNodeKind(c.type)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              ) : <p className="repo-legend-empty">No components match.</p>}
            </div>
            <div className="repo-nav-section">
              <h3 className="repo-nav-title">Files<span className="blast-count">{fileHits.length}</span></h3>
              {fileHits.length ? (
                <ul className="blast-results">
                  {fileHits.map((f) => (
                    <li key={f.path}>
                      <button
                        type="button"
                        className="blast-result file"
                        disabled={!f.ownerId}
                        title={f.ownerId ? `Owned by ${f.ownerName}` : "Not mapped to a component"}
                        onClick={() => f.ownerId && onFocusBlastNode(f.ownerId)}
                      >
                        <span className="blast-result-name">{f.path}</span>
                        <span className="blast-result-kind">{f.ownerName ?? "unmapped"}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              ) : <p className="repo-legend-empty">No files match.</p>}
            </div>
          </>
        ) : (
          <div className="repo-nav-section">
            <p className="repo-legend-empty">Start typing to search the repository.</p>
          </div>
        )}
      </>
    );
  }

  if (mode === "repo-tree") {
    const legend = ownerLegend(nodes, allFlows, repoTreeLens);
    return (
      <>
        <div className="panel-head">
          <h2>Repo Tree</h2>
          <p>The repository's tracked files, colored by the architecture node that owns each path. Click a file to inspect its component.</p>
        </div>
        <div className="repo-nav-section">
          <h3 className="repo-nav-title">Color by</h3>
          <div className="repo-lens-toggle" role="group" aria-label="Color lens">
            <button type="button" className={repoTreeLens === "c4" ? "active" : ""} onClick={() => onRepoTreeLensChange("c4")}>C4 type</button>
            <button type="button" className={repoTreeLens === "flow" ? "active" : ""} onClick={() => onRepoTreeLensChange("flow")}>Flow</button>
          </div>
        </div>
        <div className="repo-nav-section">
          <h3 className="repo-nav-title">{repoTreeLens === "c4" ? "Component types" : "Flows"}</h3>
          {legend.length ? (
            <ul className="repo-legend">
              {legend.map((entry) => (
                <li key={entry.key} className="repo-legend-item">
                  <span className="repo-legend-swatch" style={{ background: entry.color }} />
                  <span className="repo-legend-label">{entry.label}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="repo-legend-empty">No owned paths yet. Map files with <code>sourcePaths</code> on architecture nodes.</p>
          )}
        </div>
      </>
    );
  }

  if (mode === "rules") {
    const categories = ruleCategories(rules ?? []);
    return (
      <>
        <div className="panel-head">
          <h2>Rule Categories</h2>
          <p>{rules?.length ? `${rules.length} project rules` : "No rules configured."}</p>
        </div>
        <div className="entity-list rule-category-list">
          {categories.length ? categories.map((category) => (
            <button
              key={category.id}
              type="button"
              className={`entity-card rule-category-card ${activeRuleCategory === category.id ? "active" : ""}`}
              style={{ "--rule-category-accent": ruleCategoryAccent(category.id) } as React.CSSProperties}
              onClick={() => onSelectRuleCategory(category.id)}
            >
              <strong className="entity-card-title">{category.label}</strong>
              <span>{category.count} {category.count === 1 ? "rule" : "rules"} ranked by criticality and order.</span>
              <div className="release-card-badges">
                <Badge>{category.count} total</Badge>
                {category.criticalCount ? <Badge tone={ruleCriticalityTone("critical")}>{category.criticalCount} critical</Badge> : null}
              </div>
            </button>
          )) : (
            <article className="entity-card passive">
              <strong>No Rules data</strong>
              <span>Add docs/architext/data/rules.json and reference it from manifest.files.rules.</span>
            </article>
          )}
          <button
            type="button"
            className="entity-card rule-category-card rule-category-add-button"
            onClick={onAddRuleCategory}
          >
            <strong className="entity-card-title">Add Category and Rule</strong>
            <span>Create the first rule in a new user-defined category.</span>
          </button>
        </div>
      </>
    );
  }

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
              <strong className="entity-card-title">{release.name}</strong>
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

  const flowViews = compatibleFlowViewsForFlow(views, activeFlow) as View[];

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
      <div className="flow-panel-body">
        {mode === "flows" && flowViews.length ? (
          <div className="entity-list flow-projection-list">
            <h3>Flow Views</h3>
            {flowViews.map((view) => (
              <button
                key={view.id}
                type="button"
                className={`entity-card ${activeView.id === view.id ? "active" : ""}`}
                onClick={() => onSelectView(view.id)}
              >
                <strong className="entity-card-title">{view.name}</strong>
                <span>{view.summary}</span>
                <Badge>{view.type}</Badge>
              </button>
            ))}
          </div>
        ) : null}
        <div className="flow-list">
          {mode === "flows" ? <h3>Flows</h3> : null}
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
  onToggleFocus,
  onExportPdf
}: {
  transform: DiagramTransform;
  routingStyle: RoutingStyle;
  onRoutingStyleChange: (style: RoutingStyle) => void;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFit: () => void;
  onReset: () => void;
  onToggleFocus: () => void;
  onExportPdf: () => void;
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
      <button type="button" onClick={onExportPdf}>{pdfExportControlLabel}</button>
    </div>
  );
}

function SystemMap({
  view,
  nodesById,
  activeFlow,
  showStructuralConnections,
  selectedStepId,
  selectedStepDisplayIndex,
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
  selectedStepDisplayIndex: number | null;
  selectedRelationship: Extract<Selection, { kind: "relationship" }> | null;
  selectedNodeId: Id | null;
  transform: DiagramTransform;
  routingStyle: RoutingStyle;
  debugRouting: boolean;
  onSelectRelationship: (relationship: Relationship) => void;
  onSelectNode: (id: Id) => void;
}) {
  const diagramConfig = useDiagramConfig();
  const visibleNodeIds = useMemo(() => new Set(view.lanes.flatMap((lane) => lane.nodeIds)), [view]);
  const flowNodeIds = useMemo(
    () => new Set(activeFlow ? activeFlow.steps.flatMap((step) => [step.from, step.to]) : Array.from(visibleNodeIds)),
    [activeFlow, visibleNodeIds]
  );
  const structuralRelationships = useMemo(() => Array.from(visibleNodeIds).flatMap((nodeId) => {
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
  }), [nodesById, visibleNodeIds]);

  const flowRelationships = useMemo(() => {
    if (!activeFlow) return [];
    const displayIndexes = flowStepDisplayIndexes(activeFlow.steps);
    const decisionStepByTarget = new Map(activeFlow.steps.filter((step) => step.kind === "decision").map((step) => [step.to, step]));
    return activeFlow.steps.map((step, index) => {
      const displayIndex = displayIndexes.get(step.id) ?? index + 1;
      const decisionStep = step.outcome ? decisionStepByTarget.get(step.from) : null;
      const decisionPosition = decisionStep ? nodeLanePosition(view, step.from) : null;
      const branchStartSide = decisionStep && decisionPosition ? preferredDecisionBranchSide(view, decisionPosition, step.to) : undefined;
      return {
        id: step.id,
        from: decisionStep ? decisionNodeId(decisionStep.id) : step.from,
        to: step.to,
        label: `${displayIndex}. ${step.action}`,
        summary: step.summary,
        relationshipType: "flow" as const,
        stepId: decisionStep ? decisionStep.id : step.id,
        branchStepId: decisionStep ? step.id : undefined,
        flowId: activeFlow.id,
        displayIndex,
        kind: step.kind,
        returnOf: step.returnOf,
        stepKind: step.kind,
        outcome: step.outcome,
        componentFrom: step.from,
        componentTo: step.to,
        preferredStartSide: branchStartSide,
        preferredEndSide: branchStartSide && decisionPosition ? preferredDecisionBranchEndSide(view, decisionPosition, step.to, branchStartSide) : undefined
      };
    });
  }, [activeFlow, view]);

  const layout = diagramLayoutFor(view, showStructuralConnections ? structuralRelationships.length : flowRelationships.length, diagramConfig?.layout);
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
  const decisionNodes = useMemo(() => {
    if (!activeFlow) return [];
    const branchedTargets = decisionBranchTargets(activeFlow.steps);
    const displayIndexes = flowStepDisplayIndexes(activeFlow.steps);
    return activeFlow.steps
      .filter((step) => step.kind === "decision" && branchedTargets.has(step.to))
      .flatMap((step) => {
        const position = nodeLanePosition(view, step.to);
        if (!position) return [];
        const { laneIndex, rowIndex } = position;
        return [{
          id: decisionNodeId(step.id),
          action: step.action,
          componentId: step.to,
          displayIndex: displayIndexes.get(step.id) ?? 0,
          rect: {
            x: marginX + laneIndex * laneWidth + nodeWidth / 2 - 19,
            y: marginY + rowIndex * rowGap + nodeHeight + 22,
            width: 38,
            height: 38
          },
          laneIndex,
          rowIndex
        }];
      });
  }, [activeFlow, view, marginX, marginY, laneWidth, rowGap, nodeHeight]);

  const planInput = useMemo(() => ({
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
    extraNodeRects: new Map(decisionNodes.map((node) => [node.id, decisionRouteRect(node.rect)])),
    extraLaneIndexByNode: new Map(decisionNodes.map((node) => [node.id, node.laneIndex])),
    extraRowIndexByNode: new Map(decisionNodes.map((node) => [node.id, node.rowIndex])),
    style: routingStyle
  }), [
    view,
    showStructuralConnections,
    structuralRelationships,
    flowRelationships,
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
    decisionNodes,
    routingStyle
  ]);
  const planningState = usePlannedDiagram(planInput);
  const fallbackCanvas = useMemo(() => plannedCanvasFallback(planInput), [planInput]);
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
            <RoutingLoadingOverlay active={planningState.planning} phase={planningState.phase} progress={planningState.progress} />
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
    (selectedStepDisplayIndex !== null && relationship.displayIndex === selectedStepDisplayIndex) || (
      selectedRelationship?.from === relationship.from &&
      selectedRelationship.to === relationship.to &&
      selectedRelationship.stepId === relationship.stepId
    )
  );
  const orderedStructuralRelationships = [...structuralRelationships].sort((a, b) => Number(isStructuralSelected(a)) - Number(isStructuralSelected(b)));
  const orderedFlowRelationships = [...flowRelationships].sort((a, b) => Number(isFlowSelected(a)) - Number(isFlowSelected(b)));
  const visibleDecisionConnectorRoutes = !showStructuralConnections
    ? decisionNodes.flatMap((decisionNode) => {
      const componentRect = plan.nodeRects.get(decisionNode.componentId);
      return componentRect ? [decisionConnectorRoute(decisionNode, componentRect)] : [];
    })
    : [];
  const selectedEndpointNodeIds = new Set(orderedFlowRelationships.filter(isFlowSelected).flatMap((relationship) => [
    relationship.componentFrom ?? relationship.from,
    relationship.componentTo ?? relationship.to
  ]).filter((nodeId) => visibleNodeIds.has(nodeId)));

  return (
    <section className="map-shell" aria-busy={planningState.planning ? "true" : "false"}>
      <ScaledCanvasExtent width={canvasWidth} height={canvasHeight} contentWidth={contentExtent(plan).width} contentHeight={contentExtent(plan).height} transform={transform}>
        <div
          className="diagram-canvas"
          style={canvasTransformStyle(canvasWidth, canvasHeight, transform)}
        >
          <svg className="flow-lines" width={canvasWidth} height={canvasHeight} aria-hidden="false" role="group" aria-label={`${view.name} relationships`}>
          <defs>
            <marker id="arrowhead" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
            </marker>
            <marker id="arrowhead-selected" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
            </marker>
            <marker id="flow-arrowhead-selected" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
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
          {!showStructuralConnections && decisionNodes.map((decisionNode) => {
            const componentRect = plan.nodeRects.get(decisionNode.componentId);
            if (!componentRect) return null;
            const connector = decisionConnectorRoute(decisionNode, componentRect);
            const isSelected = selectedStepDisplayIndex === decisionNode.displayIndex;
            const connectorNodeType = nodesById.get(decisionNode.componentId)?.type ?? "";
            return (
              <line
                key={`${decisionNode.id}:connector`}
                className={`decision-node-connector ${connectorNodeType}${isSelected ? " selected" : ""}`}
                x1={connector.points[0].x}
                y1={connector.points[0].y}
                x2={connector.points[1].x}
                y2={connector.points[1].y}
              />
            );
          })}
          {!showStructuralConnections && orderedFlowRelationships.map((relationship, index) => {
            if (!plan.laneIndexByNode.has(relationship.from) || !plan.laneIndexByNode.has(relationship.to)) {
              return null;
            }
            const route = flowRoutes.get(relationship.id);
            if (!route) return null;
            const isSelected = isFlowSelected(relationship);
            const outcomePoint = relationship.outcome ? outcomeLabelPoint(route) : null;
            const allDisplayRoutes = routingStyle === "orthogonal"
              ? visibleDecisionConnectorRoutes.concat(orderedFlowRelationships
                  .map((candidate) => flowRoutes.get(candidate.id))
                  .filter(Boolean))
              : [];
            const routeD = routingStyle === "orthogonal"
              ? pathToSvgWithHops(route.points, allDisplayRoutes)
              : route.d;
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
                {relationship.outcome ? (
                  <path
                    className="flow-line decision-branch-line"
                    d={routeD}
                    markerEnd={isSelected ? "url(#flow-arrowhead-selected)" : "url(#arrowhead)"}
                  />
                ) : (
                  <StepRoute
                    className={stepRouteClassName("flow")}
                    lineClassName="flow-line"
                  markerClassName="flow-step-dot"
                  labelClassName="flow-step-label"
                  d={routeD}
                    markerEnd={isSelected ? "url(#flow-arrowhead-selected)" : "url(#arrowhead)"}
                    labelX={route.labelX}
                    labelY={route.labelY}
                    label={relationship.displayIndex}
                  />
                )}
                {relationship.outcome ? (
                  <g className="flow-outcome-label" transform={`translate(${outcomePoint?.x ?? route.labelX} ${outcomePoint?.y ?? route.labelY})`}>
                    <rect x={-Math.max(28, relationship.outcome.length * 4 + 8)} y="-11" width={Math.max(56, relationship.outcome.length * 8 + 16)} height="20" rx="10" />
                    <text y="2">{relationship.outcome}</text>
                  </g>
                ) : null}
              </g>
            );
          })}
          {debugRouting ? <RoutingDebugGeometry plan={plan} relationships={showStructuralConnections ? structuralRelationships : flowRelationships} /> : null}
          </svg>
          {!showStructuralConnections && decisionNodes.map((decisionNode) => {
            const isSelected = selectedStepDisplayIndex === decisionNode.displayIndex;
            return (
              <div
                key={decisionNode.id}
                className={`decision-node ${isSelected ? "selected" : ""}`}
                style={{
                  left: decisionNode.rect.x,
                  top: decisionNode.rect.y,
                  width: decisionNode.rect.width,
                  height: decisionNode.rect.height
                }}
                title={`${decisionNode.displayIndex}. ${decisionNode.action}`}
              >
                <DiagramIcon icon="decision" />
                <span>{decisionNode.displayIndex}</span>
              </div>
            );
          })}
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
            const rect = plan.nodeRects.get(node.id);
            if (!rect) return null;
            const isActive = flowNodeIds.has(node.id);
            const isSelected = selectedNodeId === node.id;
            const isEndpointSelected = selectedEndpointNodeIds.has(node.id);
            return (
              <button
                key={node.id}
                type="button"
                className={`node-card ${node.type} ${isActive ? "in-flow" : ""} ${isSelected ? "selected" : ""} ${isEndpointSelected ? "selected-endpoint" : ""}`}
                style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
                onClick={() => onSelectNode(node.id)}
              >
                <span className="node-card-title">
                  <DiagramIcon icon={iconForNodeType(node.type)} className="node-icon" />
                  <strong>{node.name}</strong>
                </span>
                <span>{node.type}</span>
              </button>
            );
          })}
          <RoutingLoadingOverlay active={planningState.planning} phase={planningState.phase} progress={planningState.progress} />
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

function topQualityCosts(route: DiagramRoute) {
  return Object.entries(route.qualityCosts ?? {})
    .filter(([, value]) => typeof value === "number" && value !== 0)
    .sort(([, left], [, right]) => Math.abs(right as number) - Math.abs(left as number))
    .slice(0, 5);
}

function RoutingDebugPanel({ plan, relationships }: { plan: DiagramPlan; relationships: Relationship[] }) {
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
          {warnings.slice(0, 8).map((warning, index: number) => (
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

function RoutingDebugGeometry({ plan, relationships }: { plan: DiagramPlan; relationships: Relationship[] }) {
  return (
    <g className="routing-debug-geometry" aria-hidden="true">
      {[...plan.nodeRects.entries()].map(([nodeId, rect]) => (
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
        const points = route.points ?? [];
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
            {points.map((point, index: number) => (
              <circle
                key={`point-${relationship.id}-${index}`}
                className="routing-debug-point"
                cx={point.x}
                cy={point.y}
                r={index === 0 || index === points.length - 1 ? 4 : 2.5}
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
  const rawNodeIds = useMemo(() => view.lanes.flatMap((lane) => lane.nodeIds), [view]);
  const allNodeIds = useMemo(() => Array.from(new Set(rawNodeIds)), [rawNodeIds]);
  const duplicateNodeIds = useMemo(() => Array.from(
    rawNodeIds.reduce((counts, nodeId) => counts.set(nodeId, (counts.get(nodeId) ?? 0) + 1), new Map<Id, number>())
  ).filter(([, count]) => count > 1).map(([nodeId]) => nodeId), [rawNodeIds]);
  const documentWarnings = useMemo(() => duplicateNodeIds.map((nodeId) => ({
    code: "duplicate-c4-node",
    nodeId,
    viewId: view.id,
    message: `${nodeId} appears more than once in ${view.name}; rendered once.`
  })), [duplicateNodeIds, view.id, view.name]);
  const visibleNodeIds = useMemo(() => new Set(allNodeIds), [allNodeIds]);
  const relationships = useMemo(() => allNodeIds.flatMap((nodeId) => {
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
  }), [allNodeIds, nodesById, visibleNodeIds]);
  const planInput = useMemo(() => ({
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
  }), [
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
    routingStyle
  ]);
  const planningState = usePlannedDiagram(planInput);
  const fallbackCanvas = useMemo(() => plannedCanvasFallback(planInput), [planInput]);
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
            <RoutingLoadingOverlay active={planningState.planning} phase={planningState.phase} progress={planningState.progress} />
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
      <ScaledCanvasExtent width={canvasWidth} height={canvasHeight} contentWidth={contentExtent(plan).width} contentHeight={contentExtent(plan).height} transform={transform}>
        <div
          className={`c4-canvas ${view.type}`}
          style={canvasTransformStyle(canvasWidth, canvasHeight, transform)}
        >
          <svg className="flow-lines c4-lines" width={canvasWidth} height={canvasHeight} role="group" aria-label={`${view.name} structural relationships`}>
          <defs>
            <marker id="c4-arrowhead" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
            </marker>
            <marker id="c4-arrowhead-selected" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
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
            const rect = plan.nodeRects.get(nodeId);
            if (!rect) return null;
            return (
              <button
                key={node.id}
                type="button"
                className={`c4-node ${node.type} ${selectedNodeId === node.id ? "selected" : ""}`}
                style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
                onClick={() => onSelectNode(node.id)}
                aria-label={`${node.name}, ${node.type}. ${node.summary}`}
              >
                <strong>{node.name}</strong>
                <span>{node.type}</span>
                <small>{node.summary}</small>
              </button>
            );
          })}
          <RoutingLoadingOverlay active={planningState.planning} phase={planningState.phase} progress={planningState.progress} />
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
  const sequenceConfig = useDiagramConfig()?.sequence;
  const participantIds = Array.from(new Set(activeFlow.steps.flatMap((step) => [step.from, step.to])));
  const participantWidth = sequenceConfig?.participantWidth ?? 146;
  const rowHeight = sequenceConfig?.rowHeight ?? 56;
  const marginX = sequenceConfig?.marginX ?? 28;
  const headerY = 18;
  const messageStartY = 68;
  const width = marginX * 2 + participantIds.length * participantWidth;
  const height = messageStartY + activeFlow.steps.length * rowHeight + 38;
  const xFor = (id: Id) => marginX + participantIds.indexOf(id) * participantWidth + participantWidth / 2;
  const yForStepIndex = (index: number) => messageStartY + index * rowHeight;
  const stepIndexById = new Map(activeFlow.steps.map((step, index) => [step.id, index]));
  const activationBars = (sequenceActivationSpans(activeFlow.steps, rowHeight) as SequenceActivationSpan[]).map((span) => ({
    ...span,
    x: xFor(span.participantId) + span.depth * 8 - 5,
    y: messageStartY + span.y1,
    height: Math.max(18, span.y2 - span.y1)
  }));
  const sequenceFrames = (activeFlow.sequenceFrames ?? []).flatMap((frame) => {
    const indexes = frame.stepIds
      .map((stepId) => stepIndexById.get(stepId))
      .filter((index): index is number => index !== undefined);
    if (indexes.length === 0) return [];
    const participantIndexes = frame.stepIds.flatMap((stepId) => {
      const step = activeFlow.steps[stepIndexById.get(stepId) ?? -1];
      return step ? [participantIds.indexOf(step.from), participantIds.indexOf(step.to)] : [];
    }).filter((index) => index >= 0);
    const minParticipant = Math.min(...participantIndexes);
    const maxParticipant = Math.max(...participantIndexes);
    const minIndex = Math.min(...indexes);
    const maxIndex = Math.max(...indexes);
    const x = marginX + minParticipant * participantWidth + 8;
    const frameWidth = (maxParticipant - minParticipant + 1) * participantWidth - 16;
    const y = yForStepIndex(minIndex) - 30;
    const frameHeight = yForStepIndex(maxIndex) - y + 34;
    return [{ ...frame, x, y, width: frameWidth, height: frameHeight }];
  });
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
            <marker id="sequence-arrowhead" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
            </marker>
            <marker id="sequence-arrowhead-response" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
            </marker>
            <marker id="sequence-arrowhead-persistence" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
            </marker>
            <marker id="sequence-arrowhead-selected" markerWidth="4" markerHeight="4" refX="3" refY="2" orient="auto">
              <path d="M 0 0 L 4 2 L 0 4 z" />
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
          {sequenceFrames.map((frame) => (
            <g key={frame.id} className={`sequence-frame ${frame.type}`}>
              <rect x={frame.x} y={frame.y} width={frame.width} height={frame.height} rx="3" />
              <text x={frame.x + 8} y={frame.y + 14}>{frame.type}: {frame.label}</text>
            </g>
          ))}
          {activationBars.map((bar) => (
            <rect
              key={bar.id}
              className="sequence-activation-bar"
              x={bar.x}
              y={bar.y}
              width="10"
              height={bar.height}
              rx="1.5"
            />
          ))}
          {orderedStepMessages.map(({ step, index }) => {
            const fromX = xFor(step.from);
            const toX = xFor(step.to);
            const y = messageStartY + index * rowHeight;
            const midX = (fromX + toX) / 2;
            const dataLabel = step.data.map((id) => dataById.get(id)?.name ?? id).join(", ");
            const messageKind = sequenceStepMessageKind(step, fromX, toX);
            const markerId = selectedStepId === step.id
              ? "sequence-arrowhead-selected"
              : messageKind === "return"
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
                <StepRoute
                  className={stepRouteClassName("sequence")}
                  lineClassName={`sequence-line ${messageKind}`}
                  markerClassName="sequence-step-dot"
                  labelClassName="sequence-step-label"
                  x1={fromX}
                  y1={y}
                  x2={toX}
                  y2={y}
                  markerEnd={`url(#${markerId})`}
                  labelX={midX}
                  labelY={y}
                  label={index + 1}
                />
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
  notes,
  onSaveNote,
  onDeleteNote,
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
  notes: ElementNote[];
  onSaveNote: (note: ElementNote) => Promise<void>;
  onDeleteNote: (id: Id) => Promise<void>;
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
        <NotesSection targetKind="node" targetId={node.id} notes={notes} onSave={onSaveNote} onDelete={onDeleteNote} />
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
      <NotesSection targetKind="flow" targetId={flow.id} notes={notes} onSave={onSaveNote} onDelete={onDeleteNote} />
    </DetailShell>
  );
}

function DetailShell({
  eyebrow,
  title,
  summary,
  children,
  sections = ["Summary", "Runtime", "Interfaces", "Data", "Security", "Observability", "Risks", "Decisions", "Verification"]
}: {
  eyebrow: string;
  title: string;
  summary: string;
  children: React.ReactNode;
  sections?: string[];
}) {
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
