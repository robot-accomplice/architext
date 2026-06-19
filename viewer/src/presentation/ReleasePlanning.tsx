import { useEffect, useRef, useState } from "react";
import { mutationFetch } from "../adapters/mutationAuth.js";
import { releasePlanActionDisabled, releasePlanProposalPayload } from "./releasePlanningModel.js";
import { releaseItems } from "./releaseTruth.js";
import type {
  Id,
  ReleaseDetail,
  ReleaseItemKind,
  ReleaseModel,
  ReleaseSummary,
  RoadmapItem
} from "../domain/architectureTypes.js";

export type ReleasePlanningScope = keyof ReleaseDetail["scope"];

type AdHocPlanningItem = {
  id: Id;
  persisted?: boolean;
  title: string;
  summary?: string;
  kind: ReleaseItemKind;
  priority: "critical" | "high" | "medium" | "low";
  section: string;
  scope: ReleasePlanningScope;
};

type ReleasePlanPreview = {
  release?: ReleaseSummary;
  changes?: {
    releaseFile: {
      action: string;
      file: string;
      name: string;
    };
    releaseIndex: {
      action: string;
      currentReleaseId: Id;
      releaseCount: number;
    };
    roadmap: {
      add: number;
      retarget: number;
      unchanged: number;
    };
  };
  validation?: {
    ok: boolean;
    output: string;
  };
};

const releasePlanningScopeLabels: Record<ReleasePlanningScope, string> = {
  required: "Required",
  planned: "Planned",
  stretch: "Stretch",
  deferred: "Deferred",
  outOfScope: "Out of scope"
};

function nextMinorVersionFromReleases(releases: ReleaseSummary[]): string {
  const versions = releases
    .map((release) => release.version.match(/^(\d+)\.(\d+)\.(\d+)$/))
    .filter((match): match is RegExpMatchArray => Boolean(match))
    .map((match) => match.slice(1).map(Number))
    .sort((left, right) => left[0] - right[0] || left[1] - right[1] || left[2] - right[2]);
  const latest = versions.at(-1) ?? [0, 0, 0];
  return `${latest[0]}.${latest[1] + 1}.0`;
}

function sortRoadmapItems(items: RoadmapItem[]): RoadmapItem[] {
  const priorityRank = { critical: 0, high: 1, medium: 2, low: 3 };
  return [...items].sort((left, right) => {
    const section = left.section.localeCompare(right.section);
    if (section !== 0) return section;
    return (priorityRank[left.priority ?? "medium"] ?? 2) - (priorityRank[right.priority ?? "medium"] ?? 2)
      || left.title.localeCompare(right.title);
  });
}

function planningCandidateItems(items: RoadmapItem[], activeReleaseId?: Id): RoadmapItem[] {
  return items.filter((item) => (
    item.targetReleaseId === activeReleaseId
    || item.status === "deferred"
    || !item.targetReleaseId
  ));
}

function editableReleaseScope(detail: ReleaseDetail | null) {
  if (!detail) return null;
  const itemScopes = new Map<Id, ReleasePlanningScope>([
    ...detail.scope.required.map((item) => [item.id, "required"] as const),
    ...detail.scope.planned.map((item) => [item.id, "planned"] as const),
    ...detail.scope.stretch.map((item) => [item.id, "stretch"] as const),
    ...detail.scope.deferred.map((item) => [item.id, "deferred"] as const),
    ...detail.scope.outOfScope.map((item) => [item.id, "outOfScope"] as const)
  ]);
  const items = releaseItems(detail).filter((item) => item.status !== "cut");
  return {
    selectedRoadmapIds: items.filter((item) => item.source !== "ad-hoc").map((item) => item.id),
    itemScopes,
    adHocItems: items
      .filter((item) => item.source === "ad-hoc")
      .map((item) => ({
        id: item.id,
        persisted: true,
        title: item.title,
        summary: item.summary === item.title ? undefined : item.summary,
        kind: item.kind,
        priority: item.priority ?? "medium",
        section: detail.workstreams.find((workstream) => workstream.id === item.workstreamId)?.name ?? "Ad hoc",
        scope: itemScopes.get(item.id) ?? "planned"
      }))
  };
}

export function ReleasePlanningPanel({
  releaseIndex,
  roadmapItems,
  activeReleaseSummary,
  activeReleaseDetail,
  onApproved,
  onEditingChange
}: {
  releaseIndex: ReleaseModel["index"];
  roadmapItems: RoadmapItem[];
  activeReleaseSummary: ReleaseSummary | null;
  activeReleaseDetail: ReleaseDetail | null;
  onApproved: () => Promise<void>;
  onEditingChange: (editing: boolean) => void;
}) {
  const [selectedIds, setSelectedIds] = useState<Set<Id>>(new Set());
  const [adHocItems, setAdHocItems] = useState<AdHocPlanningItem[]>([]);
  const [version, setVersion] = useState(() => nextMinorVersionFromReleases(releaseIndex.releases));
  const [theme, setTheme] = useState("");
  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");
  const [kind, setKind] = useState<ReleaseItemKind>("feature");
  const [priority, setPriority] = useState<AdHocPlanningItem["priority"]>("medium");
  const [section, setSection] = useState("Release Planning");
  const [scope, setScope] = useState<ReleasePlanningScope>("planned");
  const [itemScopes, setItemScopes] = useState<Record<Id, ReleasePlanningScope>>({});
  const [message, setMessage] = useState("");
  const [preview, setPreview] = useState<ReleasePlanPreview | null>(null);
  const [pending, setPending] = useState(false);
  const [adHocOpen, setAdHocOpen] = useState(false);
  const adHocDrawerRef = useRef<HTMLDivElement | null>(null);
  const markEditing = () => onEditingChange(true);
  const editableRelease = activeReleaseSummary?.status !== "completed" ? activeReleaseSummary : null;
  const candidateRoadmapItems = sortRoadmapItems(planningCandidateItems(roadmapItems, activeReleaseDetail?.id));
  const candidateRoadmapIds = new Set(candidateRoadmapItems.map((item) => item.id));
  const selectedRoadmapIds = [...selectedIds].filter((id) => candidateRoadmapIds.has(id));
  const selectedCount = selectedRoadmapIds.length + adHocItems.length;
  const planningItems = [
    ...candidateRoadmapItems.map((item) => ({
      ...item,
      scope: itemScopes[item.id] ?? "planned",
      selected: selectedIds.has(item.id),
      source: "roadmap" as const
    })),
    ...adHocItems.map((item) => ({
      id: item.id,
      title: item.title,
      summary: item.summary ?? "",
      kind: item.kind,
      status: "planned" as const,
      priority: item.priority,
      section: item.section,
      scope: item.scope,
      selected: true,
      source: "ad-hoc" as const
    }))
  ];

  useEffect(() => {
    if (!adHocOpen) return;
    window.requestAnimationFrame(() => {
      adHocDrawerRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
    });
  }, [adHocOpen]);

  useEffect(() => {
    if (!editableRelease || !activeReleaseDetail) {
      setVersion(nextMinorVersionFromReleases(releaseIndex.releases));
      setTheme("");
      setSelectedIds(new Set());
      setAdHocItems([]);
      setItemScopes({});
      setPreview(null);
      setMessage("");
      return;
    }
    const scope = editableReleaseScope(activeReleaseDetail);
    setVersion(activeReleaseDetail.version);
    setTheme("");
    setSelectedIds(new Set(scope?.selectedRoadmapIds ?? []));
    setAdHocItems(scope?.adHocItems ?? []);
    setItemScopes(Object.fromEntries(scope?.itemScopes ?? []));
    setPreview(null);
    setMessage("");
    onEditingChange(false);
  }, [activeReleaseDetail?.id, activeReleaseSummary?.status, releaseIndex.releases, editableRelease]);

  const proposalPayload = (dryRun: boolean, action: "preview" | "approve" | "save-draft") => (
    releasePlanProposalPayload({ dryRun, action, version, theme, selectedRoadmapIds, itemScopes, adHocItems })
  );

  const toggleRoadmapItem = (id: Id) => {
    markEditing();
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const togglePlanningItem = (item: { id: Id; source: "roadmap" | "ad-hoc" }) => {
    markEditing();
    setPreview(null);
    if (item.source === "ad-hoc") {
      setAdHocItems((current) => current.filter((candidate) => candidate.id !== item.id));
      return;
    }
    toggleRoadmapItem(item.id);
  };

  const changePlanningItemScope = (item: { id: Id; source: "roadmap" | "ad-hoc" }, nextScope: ReleasePlanningScope) => {
    markEditing();
    setPreview(null);
    if (item.source === "ad-hoc") {
      setAdHocItems((current) => current.map((candidate) => (
        candidate.id === item.id ? { ...candidate, scope: nextScope } : candidate
      )));
      return;
    }
    setItemScopes((current) => ({ ...current, [item.id]: nextScope }));
  };

  const addAdHocItem = () => {
    if (!title.trim()) return;
    markEditing();
    setAdHocItems((current) => [
      ...current,
      {
        id: `ad-hoc-${Date.now()}`,
        title: title.trim(),
        ...(summary.trim() ? { summary: summary.trim() } : {}),
        kind,
        priority,
        section: section.trim() || "Ad hoc",
        scope
      }
    ]);
    setTitle("");
    setSummary("");
    setScope("planned");
    setAdHocOpen(false);
    setPreview(null);
  };

  const submitPlan = async (action: "preview" | "approve" | "save-draft") => {
    const dryRun = action === "preview";
    setPending(true);
    setMessage("");
    try {
      const response = await mutationFetch("/api/release-plans", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(proposalPayload(dryRun, action))
      });
      const responseText = await response.text();
      let payload: ReleasePlanPreview & { error?: string; ok?: boolean } = {};
      try {
        payload = responseText ? JSON.parse(responseText) as ReleasePlanPreview & { error?: string; ok?: boolean } : {};
      } catch {
        throw new Error(`Release planning API did not return JSON. Start Architext with architext serve, or use the configured Vite dev server.`);
      }
      if (!response.ok || payload.ok === false) throw new Error(payload.error ?? `Release plan request failed: ${response.status} ${response.statusText || "Unknown error"}`);
      if (dryRun) {
        setPreview(payload);
        setMessage(`Preview ready for ${payload.release?.name ?? version}.`);
      } else {
        setPreview(null);
        setMessage(action === "save-draft"
          ? `Saved draft ${payload.release?.name ?? version}.`
          : `Created ${payload.release?.name ?? version}.`);
        await onApproved();
        onEditingChange(false);
      }
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(false);
    }
  };

  return (
    <section className="release-section release-section-wide release-planning-panel">
      <div className="release-planning-head">
        <div>
          <h3>{editableRelease ? `Edit ${editableRelease.name}` : "Plan next release"}</h3>
          <p>{editableRelease ? "Update this unreleased plan, or approve it when the scope is ready." : "Select roadmap work, add ad hoc items, then save a draft or approve one next-release plan."}</p>
        </div>
        <div className="release-planning-fields">
          <label>
            Version
            <input value={version} onChange={(event) => {
              markEditing();
              setVersion(event.target.value);
              setPreview(null);
            }} />
          </label>
          <label>
            Theme
            <input value={theme} onChange={(event) => {
              markEditing();
              setTheme(event.target.value);
              setPreview(null);
            }} placeholder="Optional" />
          </label>
        </div>
      </div>

      <div className="release-planning-grid">
        <div className="release-planning-roadmap">
          {planningItems.map((item) => (
            <label key={item.id} className="release-planning-option">
              <input
                type="checkbox"
                checked={item.selected}
                onChange={() => togglePlanningItem(item)}
              />
              <span>
                <strong>{item.title}</strong>
                <small>{releasePlanningScopeLabels[item.scope]} · {item.section} · {item.kind} · {item.priority ?? "medium"} priority · {item.source}</small>
                {item.summary ? <em>{item.summary}</em> : null}
              </span>
              {item.selected ? (
                <select
                  aria-label={`${item.title} scope`}
                  value={item.scope}
                  onChange={(event) => changePlanningItemScope(item, event.target.value as ReleasePlanningScope)}
                  onClick={(event) => event.stopPropagation()}
                >
                  {Object.entries(releasePlanningScopeLabels).map(([value, label]) => (
                    <option key={value} value={value}>{label}</option>
                  ))}
                </select>
              ) : null}
            </label>
          ))}
        </div>
      </div>

      <div className="release-planning-ad-hoc-footer" ref={adHocDrawerRef}>
        <button type="button" className="release-planning-add-button" onClick={() => setAdHocOpen((current) => !current)}>
          {adHocOpen ? "Close new item" : "Add new item"}
        </button>
        {adHocOpen ? (
          <div className="release-planning-ad-hoc">
            <strong>New release item</strong>
            <input value={title} onChange={(event) => {
              markEditing();
              setTitle(event.target.value);
            }} placeholder="Title" />
            <input value={summary} onChange={(event) => {
              markEditing();
              setSummary(event.target.value);
            }} placeholder="Summary (optional)" />
            <div className="release-planning-inline-fields">
              <select value={kind} onChange={(event) => {
                markEditing();
                setKind(event.target.value as ReleaseItemKind);
              }}>
                <option value="feature">Feature</option>
                <option value="bug-fix">Bug fix</option>
                <option value="documentation">Documentation</option>
                <option value="architecture">Architecture</option>
                <option value="test">Test</option>
                <option value="chore">Chore</option>
              </select>
              <select value={priority} onChange={(event) => {
                markEditing();
                setPriority(event.target.value as AdHocPlanningItem["priority"]);
              }}>
                <option value="critical">Critical</option>
                <option value="high">High</option>
                <option value="medium">Medium</option>
                <option value="low">Low</option>
              </select>
              <select value={scope} onChange={(event) => {
                markEditing();
                setScope(event.target.value as ReleasePlanningScope);
              }}>
                {Object.entries(releasePlanningScopeLabels).map(([value, label]) => (
                  <option key={value} value={value}>{label}</option>
                ))}
              </select>
              <input value={section} onChange={(event) => {
                markEditing();
                setSection(event.target.value);
              }} placeholder="Section" />
            </div>
            <div className="release-planning-form-actions">
              <button type="button" onClick={addAdHocItem} disabled={!title.trim()}>
                Add and select
              </button>
              <button type="button" onClick={() => setAdHocOpen(false)}>
                Cancel
              </button>
            </div>
          </div>
        ) : null}
      </div>

      {preview?.changes ? (
        <div className="release-planning-preview">
          <strong>Preview</strong>
          <span>{preview.changes.releaseFile.action} {preview.changes.releaseFile.file}</span>
          <span>{preview.changes.releaseIndex.action}; current release becomes {preview.changes.releaseIndex.currentReleaseId}</span>
          <span>Roadmap: {preview.changes.roadmap.add} added · {preview.changes.roadmap.retarget} retargeted · {preview.changes.roadmap.unchanged} unchanged</span>
          <span>{preview.validation?.output ?? "Preview passed."}</span>
        </div>
      ) : null}

      <div className="release-planning-actions">
        <span>{selectedCount} selected items</span>
        {message ? <span className="release-planning-message">{message}</span> : null}
        <button type="button" onClick={() => submitPlan("preview")} disabled={releasePlanActionDisabled({ pending, version, selectedCount })}>
          {pending ? "Working..." : "Preview changes"}
        </button>
        <button type="button" className="primary-action" onClick={() => submitPlan("save-draft")} disabled={releasePlanActionDisabled({ pending, version, selectedCount })}>
          Save draft
        </button>
        <button type="button" className="approve-action" onClick={() => submitPlan("approve")} disabled={releasePlanActionDisabled({ pending, version, selectedCount })}>
          Approve release plan
        </button>
      </div>
    </section>
  );
}

export function ReleasePlanningWorkspace({
  releases,
  roadmapItems,
  activeReleaseSummary,
  activeReleaseDetail,
  onReleasePlanApproved,
  onEditingChange
}: {
  releases?: ReleaseModel;
  roadmapItems?: RoadmapItem[];
  activeReleaseSummary: ReleaseSummary | null;
  activeReleaseDetail: ReleaseDetail | null;
  onReleasePlanApproved: () => Promise<void>;
  onEditingChange: (editing: boolean) => void;
}) {
  if (!releases || !roadmapItems) {
    return (
      <section className="release-truth-empty">
        <h2>Release Planning</h2>
        <p>Add Release Truth and roadmap data before creating a release plan.</p>
      </section>
    );
  }

  return (
    <section className="release-truth-workspace release-planning-workspace">
      <header className="release-hero">
        <div>
          <p className="eyebrow">Release Planning</p>
          <h2>Plan the next release</h2>
          <p>Select roadmap work, add ad hoc items, preview the file and roadmap changes, then approve the plan into Release Truth.</p>
        </div>
      </header>
      <ReleasePlanningPanel
        releaseIndex={releases.index}
        roadmapItems={roadmapItems}
        activeReleaseSummary={activeReleaseSummary}
        activeReleaseDetail={activeReleaseDetail}
        onApproved={onReleasePlanApproved}
        onEditingChange={onEditingChange}
      />
    </section>
  );
}
