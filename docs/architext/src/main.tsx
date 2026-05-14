import React, { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import "./styles.css";

type Id = string;

type Manifest = {
  schemaVersion: string;
  project: {
    id: Id;
    name: string;
    summary: string;
  };
  generatedAt: string;
  defaultViewId: Id;
  files: {
    nodes: string;
    flows: string;
    views: string;
    dataClassification: string;
    decisions: string;
    risks: string;
    glossary: string;
  };
  notes: string[];
};

type NodeType =
  | "actor"
  | "software-system"
  | "client"
  | "service"
  | "module"
  | "worker"
  | "queue"
  | "data-store"
  | "external-service"
  | "deployment-unit"
  | "trust-boundary";

type ArchNode = {
  id: Id;
  type: NodeType;
  name: string;
  summary: string;
  responsibilities: string[];
  owner: string;
  sourcePaths: string[];
  runtime: string;
  interfaces: string[];
  dependencies: Id[];
  dataHandled: Id[];
  security: string[];
  observability: string[];
  relatedFlows: Id[];
  relatedDecisions: Id[];
  knownRisks: Id[];
  verification: string[];
};

type FlowStep = {
  id: Id;
  from: Id;
  to: Id;
  action: Id;
  summary: string;
  data: Id[];
};

type Flow = {
  id: Id;
  name: string;
  status: "planned" | "partial" | "implemented";
  summary: string;
  trigger: string;
  actors: Id[];
  steps: FlowStep[];
  guarantees: string[];
  failureBehavior: string[];
  observability: string[];
  verification: string[];
  knownGaps: string[];
};

type View = {
  id: Id;
  name: string;
  type: string;
  summary: string;
  lanes: Array<{
    id: Id;
    name: string;
    nodeIds: Id[];
  }>;
};

type DataClass = {
  id: Id;
  name: string;
  sensitivity: "low" | "medium" | "high" | "critical";
  handling: string;
};

type Decision = {
  id: Id;
  status: string;
  title: string;
  context: string;
  decision: string;
  consequences: string[];
  relatedNodes: Id[];
  relatedFlows: Id[];
};

type Risk = {
  id: Id;
  title: string;
  category: string;
  severity: "low" | "medium" | "high" | "critical";
  status: string;
  summary: string;
  mitigations: string[];
  relatedNodes: Id[];
  relatedFlows: Id[];
};

type Model = {
  manifest: Manifest;
  nodes: ArchNode[];
  flows: Flow[];
  views: View[];
  dataClasses: DataClass[];
  decisions: Decision[];
  risks: Risk[];
};

type Selection =
  | { kind: "node"; id: Id }
  | { kind: "flow"; id: Id }
  | { kind: "step"; flowId: Id; stepId: Id }
  | { kind: "relationship"; from: Id; to: Id; label: string; relationshipType: "flow" | "structural"; stepId?: Id; flowId?: Id };

type Mode = "flows" | "sequence" | "c4" | "deployment" | "data-risks";
type DiagramTransform = {
  zoom: number;
  focused: boolean;
};

type ViewportSize = {
  width: number;
  height: number;
};

type Relationship = {
  id: Id;
  from: Id;
  to: Id;
  label: string;
  summary: string;
  relationshipType: "flow" | "structural";
  stepId?: Id;
  flowId?: Id;
};

const modeLabels: Record<Mode, string> = {
  flows: "Flows",
  sequence: "Sequence",
  c4: "C4",
  deployment: "Deployment",
  "data-risks": "Data/Risks"
};

const statusLabels: Record<Flow["status"], string> = {
  implemented: "Implemented",
  partial: "Partial",
  planned: "Planned"
};

const modeViewTypes: Record<Mode, string[]> = {
  flows: ["system-map", "flow-explorer", "dataflow"],
  sequence: ["sequence"],
  c4: ["c4-context", "c4-container", "c4-component"],
  deployment: ["deployment"],
  "data-risks": ["risk-overlay", "dataflow"]
};

function modeForView(view: View | undefined): Mode {
  if (!view) return "flows";
  if (view.type === "sequence") return "sequence";
  if (view.type.startsWith("c4-")) return "c4";
  if (view.type === "deployment") return "deployment";
  if (view.type === "risk-overlay") return "data-risks";
  return "flows";
}

function defaultViewForMode(mode: Mode, views: View[], fallback: View): View {
  const types = modeViewTypes[mode];
  return views.find((view) => types.includes(view.type)) ?? fallback;
}

function relationshipLabel(from: ArchNode | undefined, to: ArchNode | undefined): string {
  if (!from || !to) return "relates to";
  if (to.type === "data-store") return "reads/writes";
  if (to.type === "queue") return "publishes";
  if (to.type === "external-service") return "uses";
  if (from.type === "actor") return "uses";
  return "depends on";
}

async function fetchJson<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) {
    throw new Error(`Failed to load ${path}: ${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
}

function validateModel(model: Model): string[] {
  const errors: string[] = [];
  const nodeIds = new Set(model.nodes.map((node) => node.id));
  const flowIds = new Set(model.flows.map((flow) => flow.id));
  const dataIds = new Set(model.dataClasses.map((item) => item.id));
  const decisionIds = new Set(model.decisions.map((item) => item.id));
  const riskIds = new Set(model.risks.map((item) => item.id));
  const viewIds = new Set(model.views.map((item) => item.id));

  const requireKnown = (id: Id, known: Set<Id>, context: string) => {
    if (!known.has(id)) errors.push(`${context} references unknown id "${id}"`);
  };

  requireKnown(model.manifest.defaultViewId, viewIds, "manifest.defaultViewId");

  for (const node of model.nodes) {
    for (const id of node.dependencies) requireKnown(id, nodeIds, `node ${node.id}.dependencies`);
    for (const id of node.dataHandled) requireKnown(id, dataIds, `node ${node.id}.dataHandled`);
    for (const id of node.relatedFlows) requireKnown(id, flowIds, `node ${node.id}.relatedFlows`);
    for (const id of node.relatedDecisions) requireKnown(id, decisionIds, `node ${node.id}.relatedDecisions`);
    for (const id of node.knownRisks) requireKnown(id, riskIds, `node ${node.id}.knownRisks`);
  }

  for (const flow of model.flows) {
    for (const id of flow.actors) requireKnown(id, nodeIds, `flow ${flow.id}.actors`);
    for (const step of flow.steps) {
      requireKnown(step.from, nodeIds, `flow ${flow.id} step ${step.id}.from`);
      requireKnown(step.to, nodeIds, `flow ${flow.id} step ${step.id}.to`);
      for (const id of step.data) requireKnown(id, dataIds, `flow ${flow.id} step ${step.id}.data`);
    }
  }

  for (const view of model.views) {
    for (const lane of view.lanes) {
      for (const id of lane.nodeIds) requireKnown(id, nodeIds, `view ${view.id} lane ${lane.id}`);
    }
  }

  return errors;
}

async function loadModel(): Promise<Model> {
  const manifest = await fetchJson<Manifest>("/data/manifest.json");
  const base = "/data/";
  const [nodes, flows, views, dataClassification, decisions, risks] = await Promise.all([
    fetchJson<{ nodes: ArchNode[] }>(base + manifest.files.nodes),
    fetchJson<{ flows: Flow[] }>(base + manifest.files.flows),
    fetchJson<{ views: View[] }>(base + manifest.files.views),
    fetchJson<{ classes: DataClass[] }>(base + manifest.files.dataClassification),
    fetchJson<{ decisions: Decision[] }>(base + manifest.files.decisions),
    fetchJson<{ risks: Risk[] }>(base + manifest.files.risks)
  ]);
  const model = {
    manifest,
    nodes: nodes.nodes,
    flows: flows.flows,
    views: views.views,
    dataClasses: dataClassification.classes,
    decisions: decisions.decisions,
    risks: risks.risks
  };
  const errors = validateModel(model);
  if (errors.length > 0) {
    throw new Error(`Architext data failed viewer validation:\n${errors.join("\n")}`);
  }
  return model;
}

function byId<T extends { id: Id }>(items: T[]): Map<Id, T> {
  return new Map(items.map((item) => [item.id, item]));
}

function Badge({ children, tone }: { children: React.ReactNode; tone?: string }) {
  return <span className={`badge ${tone ?? ""}`}>{children}</span>;
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

function App() {
  const [model, setModel] = useState<Model | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [navCollapsed, setNavCollapsed] = useState(() => localStorage.getItem("architext-left-collapsed") === "true");
  const [rightCollapsed, setRightCollapsed] = useState(() => localStorage.getItem("architext-right-collapsed") === "true");
  const [query, setQuery] = useState("");
  const [activeMode, setActiveMode] = useState<Mode>("flows");
  const [activeViewId, setActiveViewId] = useState<Id>("");
  const [activeFlowId, setActiveFlowId] = useState<Id>("");
  const [selection, setSelection] = useState<Selection | null>(null);
  const [diagramTransform, setDiagramTransform] = useState<DiagramTransform>({ zoom: 1, focused: false });
  const [riskFilter, setRiskFilter] = useState("all");
  const [stepsCollapsed, setStepsCollapsed] = useState(false);
  const [diagramViewportRef, diagramViewportSize] = useElementSize<HTMLElement>();

  useEffect(() => {
    loadModel()
      .then((loaded) => {
        setModel(loaded);
        setActiveViewId(loaded.manifest.defaultViewId);
        setActiveMode(modeForView(loaded.views.find((view) => view.id === loaded.manifest.defaultViewId)));
        setActiveFlowId(loaded.flows[0]?.id ?? "");
        setSelection({ kind: "flow", id: loaded.flows[0]?.id ?? "" });
      })
      .catch((loadError: unknown) => {
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      });
  }, []);

  useEffect(() => {
    localStorage.setItem("architext-left-collapsed", String(navCollapsed));
  }, [navCollapsed]);

  useEffect(() => {
    localStorage.setItem("architext-right-collapsed", String(rightCollapsed));
  }, [rightCollapsed]);

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
  const activeView = selectedView && modeViewTypes[activeMode].includes(selectedView.type)
    ? selectedView
    : defaultViewForMode(activeMode, model.views, fallbackView);
  const isC4View = activeMode === "c4";
  const isSequenceView = activeMode === "sequence";
  const showStepSummary = activeMode === "flows" || activeMode === "sequence" || activeMode === "deployment";
  const flowNodeIds = new Set(activeFlow.steps.flatMap((step) => [step.from, step.to]));
  const selectedNodeId = selection?.kind === "node" ? selection.id : null;
  const selectedStepId = selection?.kind === "step"
    ? selection.stepId
    : selection?.kind === "relationship"
      ? selection.stepId ?? null
      : null;
  const selectedFlowForStep = selection?.kind === "step"
    ? flowsById.get(selection.flowId)
    : selection?.kind === "relationship" && selection.flowId
      ? flowsById.get(selection.flowId)
      : null;
  const selectedStep = selectedStepId
    ? selectedFlowForStep?.steps.find((step) => step.id === selectedStepId) ?? null
    : null;

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
    return Math.min(1, Math.max(0.6, Number(nextZoom.toFixed(2))));
  };

  const switchMode = (mode: Mode) => {
    const nextView = defaultViewForMode(mode, model.views, fallbackView);
    setActiveMode(mode);
    setActiveViewId(nextView.id);
    if (diagramViewportSize.width && diagramViewportSize.height) {
      const nextZoom = fitZoomFor(mode, nextView, activeFlow);
      setDiagramTransform((value) => ({ ...value, zoom: Math.min(value.zoom, nextZoom) }));
    }
    if (mode === "c4") {
      setSelection({ kind: "node", id: nextView.lanes.flatMap((lane) => lane.nodeIds)[0] ?? model.nodes[0]?.id ?? "" });
    } else if (mode === "data-risks") {
      setSelection({ kind: "flow", id: activeFlow.id });
    }
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
          <p className="eyebrow">Architext / {model.manifest.schemaVersion}</p>
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
          <div className="panel-rail">{modeLabels[activeMode]}</div>
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
            riskFilter={riskFilter}
            onRiskFilterChange={setRiskFilter}
            onSelectFlow={(flowId) => {
              setActiveFlowId(flowId);
              setSelection({ kind: "flow", id: flowId });
            }}
            onSelectView={setC4View}
            onSelectNode={(id) => setSelection({ kind: "node", id })}
          />
        )}
      </aside>

      <main className="diagram-area">
        <section className="diagram-header">
          <div className="diagram-title-line">
            <h2>{activeView.name}</h2>
            <p>{activeView.summary}</p>
          </div>
          <DiagramControls
            transform={diagramTransform}
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
              selectedStepId={selectedStepId}
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
              onSelectNode={(id) => setSelection({ kind: "node", id })}
              onSelectRelationship={selectRelationship}
            />
          ) : (
            <SystemMap
              view={activeView}
              nodesById={nodesById}
              activeFlow={isC4View ? null : activeFlow}
              showStructuralConnections={isC4View}
              selectedStepId={selectedStepId}
              selectedRelationship={selection?.kind === "relationship" ? selection : null}
              selectedNodeId={selectedNodeId}
              transform={diagramTransform}
              onSelectNode={(id) => setSelection({ kind: "node", id })}
              onSelectRelationship={selectRelationship}
            />
          )}
        </section>

        {showStepSummary && (
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
                      className={`step-card ${selectedStepId === step.id ? "active" : ""}`}
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
          <div className="panel-rail">Details</div>
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
  riskFilter,
  onRiskFilterChange,
  onSelectFlow,
  onSelectView,
  onSelectNode
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
  riskFilter: string;
  onRiskFilterChange: (value: string) => void;
  onSelectFlow: (id: Id) => void;
  onSelectView: (id: Id) => void;
  onSelectNode: (id: Id) => void;
}) {
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
  onZoomIn,
  onZoomOut,
  onFit,
  onReset,
  onToggleFocus
}: {
  transform: DiagramTransform;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFit: () => void;
  onReset: () => void;
  onToggleFocus: () => void;
}) {
  return (
    <div className="diagram-controls" aria-label="Diagram controls">
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
  onSelectRelationship: (relationship: Relationship) => void;
  onSelectNode: (id: Id) => void;
}) {
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  const flowNodeIds = new Set(activeFlow ? activeFlow.steps.flatMap((step) => [step.from, step.to]) : Array.from(visibleNodeIds));
  const nodeWidth = 136;
  const nodeHeight = 54;
  const laneWidth = 210;
  const rowGap = 84;
  const routeGutter = 96;
  const marginX = routeGutter + 48;
  const marginY = 76;
  const laneIndexByNode = new Map<Id, number>();
  const rowIndexByNode = new Map<Id, number>();

  view.lanes.forEach((lane, laneIndex) => {
    lane.nodeIds.forEach((nodeId, rowIndex) => {
      laneIndexByNode.set(nodeId, laneIndex);
      rowIndexByNode.set(nodeId, rowIndex);
    });
  });

  const laneHeight = Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1) * rowGap + marginY + 24;
  const canvasWidth = marginX * 2 + view.lanes.length * laneWidth + 40;
  const canvasHeight = Math.max(340, laneHeight + 64);
  const nodePosition = (nodeId: Id) => {
    const laneIndex = laneIndexByNode.get(nodeId) ?? 0;
    const rowIndex = rowIndexByNode.get(nodeId) ?? 0;
    return {
      x: marginX + laneIndex * laneWidth,
      y: marginY + rowIndex * rowGap
    };
  };

  type Side = "left" | "right" | "top" | "bottom";
  type Point = { x: number; y: number };
  type Route = { d: string; labelX: number; labelY: number; cost: number; samples: Point[] };

  const rectFor = (nodeId: Id) => {
    const position = nodePosition(nodeId);
    return {
      x: position.x,
      y: position.y,
      width: nodeWidth,
      height: nodeHeight
    };
  };

  const anchorFor = (rect: ReturnType<typeof rectFor>, side: Side): Point => {
    if (side === "left") return { x: rect.x, y: rect.y + rect.height / 2 };
    if (side === "right") return { x: rect.x + rect.width, y: rect.y + rect.height / 2 };
    if (side === "top") return { x: rect.x + rect.width / 2, y: rect.y };
    return { x: rect.x + rect.width / 2, y: rect.y + rect.height };
  };

  const cubicPoint = (start: Point, controlA: Point, controlB: Point, end: Point, t: number): Point => {
    const i = 1 - t;
    return {
      x: i ** 3 * start.x + 3 * i ** 2 * t * controlA.x + 3 * i * t ** 2 * controlB.x + t ** 3 * end.x,
      y: i ** 3 * start.y + 3 * i ** 2 * t * controlA.y + 3 * i * t ** 2 * controlB.y + t ** 3 * end.y
    };
  };

  const distanceToRect = (point: Point, rect: ReturnType<typeof rectFor>) => {
    const dx = Math.max(rect.x - point.x, 0, point.x - (rect.x + rect.width));
    const dy = Math.max(rect.y - point.y, 0, point.y - (rect.y + rect.height));
    return Math.hypot(dx, dy);
  };

  const routeCollidesWithNode = (samples: Point[], fromId: Id, toId: Id, padding = 8) => {
    const blockers = Array.from(visibleNodeIds)
      .filter((nodeId) => nodeId !== fromId && nodeId !== toId)
      .map(rectFor);
    return samples.some((point) => blockers.some((rect) =>
      point.x >= rect.x - padding &&
      point.x <= rect.x + rect.width + padding &&
      point.y >= rect.y - padding &&
      point.y <= rect.y + rect.height + padding
    ));
  };

  const routeCost = (
    start: Point,
    controlA: Point,
    controlB: Point,
    end: Point,
    label: Point,
    fromId: Id,
    toId: Id,
    usedRoutes: Point[][]
  ) => {
    const blockers = Array.from(visibleNodeIds)
      .filter((nodeId) => nodeId !== fromId && nodeId !== toId)
      .map(rectFor);
    let cost = Math.hypot(end.x - start.x, end.y - start.y);
    const samples: Point[] = [];
    let previous = start;
    for (let step = 1; step < 48; step += 1) {
      const point = cubicPoint(start, controlA, controlB, end, step / 48);
      samples.push(point);
      cost += Math.hypot(point.x - previous.x, point.y - previous.y) * 1.4;
      previous = point;
      if (point.y < 28 || point.x < 12 || point.x > canvasWidth - 12 || point.y > canvasHeight - 12) {
        cost += 12000;
      }
      for (const rect of blockers) {
        const padding = 16;
        const inside =
          point.x >= rect.x - padding &&
          point.x <= rect.x + rect.width + padding &&
          point.y >= rect.y - padding &&
          point.y <= rect.y + rect.height + padding;
        if (inside) cost += 8000;

        const distance = distanceToRect(point, rect);
        if (distance < 34) cost += (34 - distance) * 90;
      }

      for (const usedRoute of usedRoutes) {
        for (let usedIndex = 0; usedIndex < usedRoute.length; usedIndex += 3) {
          const used = usedRoute[usedIndex];
          const distance = Math.hypot(point.x - used.x, point.y - used.y);
          if (distance < 22) cost += 350;
          if (distance < 10) cost += 1400;
        }
      }
    }

    for (const rect of blockers) {
      if (distanceToRect(label, rect) < 32) {
        cost += 20000;
      }
    }

    return { cost, samples };
  };

  const nearestSample = (samples: Point[], target: Point): Point => {
    return samples.reduce((nearest, sample) => {
      const nearestDistance = Math.hypot(nearest.x - target.x, nearest.y - target.y);
      const sampleDistance = Math.hypot(sample.x - target.x, sample.y - target.y);
      return sampleDistance < nearestDistance ? sample : nearest;
    }, samples[0] ?? target);
  };

  const cubicRoute = (
    fromId: Id,
    toId: Id,
    startSide: Side,
    endSide: Side,
    controlA: Point,
    controlB: Point,
    label: Point,
    usedRoutes: Point[][]
  ): Route => {
    const start = anchorFor(rectFor(fromId), startSide);
    const end = anchorFor(rectFor(toId), endSide);
    const scored = routeCost(start, controlA, controlB, end, label, fromId, toId, usedRoutes);
    const startDirection = sideVector(startSide);
    const endDirection = sideVector(endSide);
    const targetVector = { x: end.x - start.x, y: end.y - start.y };
    const incomingVector = { x: start.x - end.x, y: start.y - end.y };
    if (startDirection.x * targetVector.x + startDirection.y * targetVector.y < 0) {
      scored.cost += 60000;
    }
    if (endDirection.x * incomingVector.x + endDirection.y * incomingVector.y < 0) {
      scored.cost += 60000;
    }
    const labelPoint = nearestSample(scored.samples, label);
    return {
      d: `M ${start.x} ${start.y} C ${controlA.x} ${controlA.y}, ${controlB.x} ${controlB.y}, ${end.x} ${end.y}`,
      labelX: labelPoint.x,
      labelY: labelPoint.y,
      cost: scored.cost,
      samples: scored.samples
    };
  };

  const tangentFor = (side: Side, bend: number): Point => {
    if (side === "left") return { x: -bend, y: 0 };
    if (side === "right") return { x: bend, y: 0 };
    if (side === "top") return { x: 0, y: -bend };
    return { x: 0, y: bend };
  };

  const sideVector = (side: Side): Point => {
    if (side === "left") return { x: -1, y: 0 };
    if (side === "right") return { x: 1, y: 0 };
    if (side === "top") return { x: 0, y: -1 };
    return { x: 0, y: 1 };
  };

  const lineSamples = (points: Point[]): Point[] => {
    const samples: Point[] = [];
    for (let index = 0; index < points.length - 1; index += 1) {
      const start = points[index];
      const end = points[index + 1];
      for (let step = 1; step <= 10; step += 1) {
        const t = step / 10;
        samples.push({
          x: start.x + (end.x - start.x) * t,
          y: start.y + (end.y - start.y) * t
        });
      }
    }
    return samples;
  };

  const routeCostFromSamples = (
    samples: Point[],
    label: Point,
    fromId: Id,
    toId: Id,
    usedRoutes: Point[][]
  ) => {
    const blockers = Array.from(visibleNodeIds)
      .filter((nodeId) => nodeId !== fromId && nodeId !== toId)
      .map(rectFor);
    let cost = 0;
    for (let index = 0; index < samples.length - 1; index += 1) {
      cost += Math.hypot(samples[index + 1].x - samples[index].x, samples[index + 1].y - samples[index].y);
    }
    for (const point of samples) {
      if (point.y < 30 || point.x < 16 || point.x > canvasWidth - 16 || point.y > canvasHeight - 16) {
        cost += 14000;
      }
      for (const rect of blockers) {
        const distance = distanceToRect(point, rect);
        if (distance < 14) cost += 12000;
        if (distance < 30) cost += (30 - distance) * 120;
      }
      for (const usedRoute of usedRoutes) {
        for (let usedIndex = 0; usedIndex < usedRoute.length; usedIndex += 2) {
          const used = usedRoute[usedIndex];
          const distance = Math.hypot(point.x - used.x, point.y - used.y);
          if (distance < 26) cost += 450;
          if (distance < 12) cost += 1600;
        }
      }
    }
    for (const rect of blockers) {
      if (distanceToRect(label, rect) < 34) cost += 24000;
    }
    return cost;
  };

  const outerGutterRoute = (fromId: Id, toId: Id, bottomCorridor: number, routeOffset: number): Route => {
    const fromRectLocal = rectFor(fromId);
    const toRectLocal = rectFor(toId);
    const fromCenterLocal = { x: fromRectLocal.x + fromRectLocal.width / 2, y: fromRectLocal.y + fromRectLocal.height / 2 };
    const toCenterLocal = { x: toRectLocal.x + toRectLocal.width / 2, y: toRectLocal.y + toRectLocal.height / 2 };
    const routeOnRight = fromCenterLocal.x >= toCenterLocal.x;
    const start = anchorFor(fromRectLocal, routeOnRight ? "right" : "left");
    const end = anchorFor(toRectLocal, "bottom");
    const requestedGutterX = routeOnRight
      ? Math.max(fromRectLocal.x + fromRectLocal.width, toRectLocal.x + toRectLocal.width) + 54 + routeOffset
      : Math.min(fromRectLocal.x, toRectLocal.x) - 54 - routeOffset;
    const gutterX = Math.min(Math.max(requestedGutterX, 28), canvasWidth - 28);
    const points = [
      start,
      { x: gutterX, y: start.y },
      { x: gutterX, y: bottomCorridor },
      { x: end.x, y: bottomCorridor },
      end
    ];
    const samples = lineSamples(points);
    return {
      d: `M ${points[0].x} ${points[0].y} L ${points[1].x} ${points[1].y} L ${points[2].x} ${points[2].y} L ${points[3].x} ${points[3].y} L ${points[4].x} ${points[4].y}`,
      labelX: (gutterX + end.x) / 2,
      labelY: bottomCorridor - 8,
      cost: routeCostFromSamples(samples, { x: (gutterX + end.x) / 2, y: bottomCorridor - 8 }, fromId, toId, []),
      samples
    };
  };

  const sideGutterRoute = (fromId: Id, toId: Id, routeOffset: number): Route => {
    const fromRectLocal = rectFor(fromId);
    const toRectLocal = rectFor(toId);
    const start = anchorFor(fromRectLocal, "left");
    const end = anchorFor(toRectLocal, "left");
    const requestedGutterX = Math.min(fromRectLocal.x, toRectLocal.x) - 54 - routeOffset;
    const gutterX = Math.max(28, requestedGutterX);
    const points = [
      start,
      { x: gutterX, y: start.y },
      { x: gutterX, y: end.y },
      end
    ];
    const samples = lineSamples(points);
    return {
      d: `M ${points[0].x} ${points[0].y} L ${points[1].x} ${points[1].y} L ${points[2].x} ${points[2].y} L ${points[3].x} ${points[3].y}`,
      labelX: gutterX,
      labelY: (start.y + end.y) / 2,
      cost: routeCostFromSamples(samples, { x: gutterX, y: (start.y + end.y) / 2 }, fromId, toId, []),
      samples
    };
  };

  const edgePath = (fromId: Id, toId: Id, index: number, pairIndex: number, usedRoutes: Point[][]) => {
    const from = nodePosition(fromId);
    const to = nodePosition(toId);
    const fromLane = laneIndexByNode.get(fromId) ?? 0;
    const toLane = laneIndexByNode.get(toId) ?? 0;
    const fromRect = rectFor(fromId);
    const toRect = rectFor(toId);
    const fromCenter = { x: from.x + nodeWidth / 2, y: from.y + nodeHeight / 2 };
    const toCenter = { x: to.x + nodeWidth / 2, y: to.y + nodeHeight / 2 };
    const mid = { x: (fromCenter.x + toCenter.x) / 2, y: (fromCenter.y + toCenter.y) / 2 };
    const candidates: Route[] = [];
    const routeOffset = pairIndex * 40 + (index % 2) * 10;
    const spanMinX = Math.min(fromCenter.x, toCenter.x);
    const spanMaxX = Math.max(fromCenter.x, toCenter.x);
    const spanBlockers = Array.from(visibleNodeIds)
      .filter((nodeId) => nodeId !== fromId && nodeId !== toId)
      .map(rectFor)
      .filter((rect) => rect.x < spanMaxX && rect.x + rect.width > spanMinX);
    const topCorridor = Math.max(
      marginY - 16,
      Math.min(fromRect.y, toRect.y, ...spanBlockers.map((rect) => rect.y)) - 42 - routeOffset
    );
    const bottomCorridor = Math.max(
      fromRect.y + nodeHeight,
      toRect.y + nodeHeight,
      ...spanBlockers.map((rect) => rect.y + rect.height)
    ) + 42 + routeOffset;

    const directStartSide: Side = Math.abs(toCenter.x - fromCenter.x) >= Math.abs(toCenter.y - fromCenter.y)
      ? toCenter.x >= fromCenter.x ? "right" : "left"
      : toCenter.y >= fromCenter.y ? "bottom" : "top";
    const directEndSide: Side = directStartSide === "right"
      ? "left"
      : directStartSide === "left"
        ? "right"
        : directStartSide === "bottom"
          ? "top"
          : "bottom";
    const directStart = anchorFor(fromRect, directStartSide);
    const directEnd = anchorFor(toRect, directEndSide);
    const directRoute = cubicRoute(
      fromId,
      toId,
      directStartSide,
      directEndSide,
      { x: (directStart.x + directEnd.x) / 2, y: directStart.y },
      { x: (directStart.x + directEnd.x) / 2, y: directEnd.y },
      mid,
      usedRoutes
    );
    if (!routeCollidesWithNode(directRoute.samples, fromId, toId, 8)) {
      directRoute.cost -= 90000;
    }
    candidates.push(directRoute);

    const rowDelta = (rowIndexByNode.get(toId) ?? 0) - (rowIndexByNode.get(fromId) ?? 0);
    if (Math.abs(rowDelta) > 1) {
      candidates.push(cubicRoute(
        fromId,
        toId,
        rowDelta < 0 ? "top" : "bottom",
        rowDelta < 0 ? "top" : "bottom",
        { x: fromCenter.x, y: rowDelta < 0 ? topCorridor : bottomCorridor },
        { x: toCenter.x, y: rowDelta < 0 ? topCorridor : bottomCorridor },
        { x: mid.x, y: rowDelta < 0 ? topCorridor : bottomCorridor },
        usedRoutes
      ));
    }

    if (Math.abs(toLane - fromLane) > 1) {
      candidates.push(cubicRoute(
        fromId,
        toId,
        "top",
        "top",
        { x: fromCenter.x, y: topCorridor },
        { x: toCenter.x, y: topCorridor },
        { x: mid.x, y: topCorridor },
        usedRoutes
      ));
    }

    (["left", "right", "top", "bottom"] as Side[]).forEach((startSide) => {
      (["left", "right", "top", "bottom"] as Side[]).forEach((endSide) => {
        const start = anchorFor(fromRect, startSide);
        const end = anchorFor(toRect, endSide);
        const bend = Math.min(180, Math.max(42, Math.hypot(end.x - start.x, end.y - start.y) * 0.28 + routeOffset));
        const startTangent = tangentFor(startSide, bend);
        const endTangent = tangentFor(endSide, bend);
        candidates.push(cubicRoute(
          fromId,
          toId,
          startSide,
          endSide,
          { x: start.x + startTangent.x, y: start.y + startTangent.y },
          { x: end.x + endTangent.x, y: end.y + endTangent.y },
          mid,
          usedRoutes
        ));
      });
    });

    if (fromLane === toLane) {
      const leftGutter = Math.min(fromRect.x, toRect.x) - 36 - routeOffset;
      const rightGutter = Math.max(fromRect.x + nodeWidth, toRect.x + nodeWidth) + 36 + routeOffset;
      candidates.push(
        cubicRoute(
          fromId,
          toId,
          "left",
          "left",
          { x: leftGutter, y: fromCenter.y },
          { x: leftGutter, y: toCenter.y },
          { x: leftGutter, y: mid.y },
          usedRoutes
        ),
        cubicRoute(
          fromId,
          toId,
          "right",
          "right",
          { x: rightGutter, y: fromCenter.y },
          { x: rightGutter, y: toCenter.y },
          { x: rightGutter, y: mid.y },
          usedRoutes
        )
      );
    }

    if (toLane > fromLane) {
      const bend = Math.max(48, Math.abs(to.x - (from.x + nodeWidth)) * 0.42 + routeOffset);
      candidates.push(cubicRoute(
        fromId,
        toId,
        "right",
        "left",
        { x: from.x + nodeWidth + bend, y: fromCenter.y },
        { x: to.x - bend, y: toCenter.y },
        mid,
        usedRoutes
      ));
    }

    if (toLane < fromLane) {
      const bend = Math.max(48, Math.abs(from.x - (to.x + nodeWidth)) * 0.42 + routeOffset);
      candidates.push(cubicRoute(
        fromId,
        toId,
        "left",
        "right",
        { x: from.x - bend, y: fromCenter.y },
        { x: to.x + nodeWidth + bend, y: toCenter.y },
        mid,
        usedRoutes
      ));
    }

    candidates.push(
      cubicRoute(
        fromId,
        toId,
        "top",
        "top",
        { x: fromCenter.x, y: topCorridor },
        { x: toCenter.x, y: topCorridor },
        { x: mid.x, y: topCorridor },
        usedRoutes
      ),
      cubicRoute(
        fromId,
        toId,
        "bottom",
        "bottom",
        { x: fromCenter.x, y: bottomCorridor },
        { x: toCenter.x, y: bottomCorridor },
        { x: mid.x, y: bottomCorridor },
        usedRoutes
      )
    );

    const topLimit = Math.min(fromRect.y, toRect.y);
    const bottomLimit = Math.max(fromRect.y + nodeHeight, toRect.y + nodeHeight);
    candidates.forEach((candidate) => {
      const travelsTop = candidate.samples.some((point) => point.y < topLimit - 4);
      const travelsBottom = candidate.samples.some((point) => point.y > bottomLimit + 4);
      if (pairIndex % 2 === 1 && travelsTop) {
        candidate.cost += 25000;
      }
      if (pairIndex % 2 === 1 && !travelsBottom) {
        candidate.cost += 4000;
      }
      if (pairIndex % 2 === 0 && travelsBottom) {
        candidate.cost += 600;
      }
    });

    return candidates.sort((a, b) => a.cost - b.cost)[0];
  };

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
          relationshipType: "structural" as const
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
      flowId: activeFlow.id
    };
  }) ?? [];

  const planRoutes = (relationships: Relationship[]) => {
    const usedRoutes: Point[][] = [];
    const pairCounts = new Map<string, number>();
    const routes = new Map<Id, Route>();

    relationships.forEach((relationship, index) => {
      if (!laneIndexByNode.has(relationship.from) || !laneIndexByNode.has(relationship.to)) {
        return;
      }

      const pairKey = [relationship.from, relationship.to].sort().join("<->");
      const pairIndex = pairCounts.get(pairKey) ?? 0;
      pairCounts.set(pairKey, pairIndex + 1);

      const route = edgePath(relationship.from, relationship.to, index, pairIndex, usedRoutes);
      routes.set(relationship.id, route);
      usedRoutes.push(route.samples);
    });

    return routes;
  };

  const structuralRoutes = planRoutes(structuralRelationships);
  const flowRoutes = planRoutes(flowRelationships);

  return (
    <section className="map-shell">
      <div
        className="diagram-canvas"
        style={{ width: canvasWidth, height: canvasHeight, transform: `scale(${transform.zoom})`, transformOrigin: "0 0" }}
      >
        <svg className="flow-lines" width={canvasWidth} height={canvasHeight} aria-hidden="false" role="group" aria-label={`${view.name} relationships`}>
          <defs>
            <marker id="arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="arrowhead-selected" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
          </defs>
          {showStructuralConnections && structuralRelationships.map((connection, index) => {
            const route = structuralRoutes.get(connection.id);
            if (!route) return null;
            const selected = selectedRelationship?.from === connection.from && selectedRelationship.to === connection.to;
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
          {!showStructuralConnections && flowRelationships.map((relationship, index) => {
            if (!laneIndexByNode.has(relationship.from) || !laneIndexByNode.has(relationship.to)) {
              return null;
            }
            const route = flowRoutes.get(relationship.id);
            if (!route) return null;
            const isSelected = selectedStepId === relationship.stepId || (
              selectedRelationship?.from === relationship.from &&
              selectedRelationship.to === relationship.to &&
              selectedRelationship.stepId === relationship.stepId
            );
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
                  markerEnd={isSelected ? "url(#arrowhead-selected)" : "url(#arrowhead)"}
                />
                <rect className="flow-step-dot" x={route.labelX - 10} y={route.labelY - 10} width="20" height="20" />
                <text className="flow-step-label" x={route.labelX} y={route.labelY + 4}>{index + 1}</text>
              </g>
            );
          })}
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
      </div>
      {activeFlow ? (
        <div className="edge-strip">
          {flowRelationships.map((relationship, index) => (
            <button
              type="button"
              className={`edge-chip ${selectedStepId === relationship.stepId ? "active" : ""}`}
              key={relationship.id}
              title={relationship.summary}
              onClick={() => onSelectRelationship(relationship)}
            >
              {index + 1}. {relationship.from} {"→"} {relationship.to}
            </button>
          ))}
          <span className="edge-count">{activeFlow.steps.length} ordered transitions</span>
        </div>
      ) : (
        <div className="edge-strip">
          <span className="edge-count">Structural connections only</span>
        </div>
      )}
    </section>
  );
}

function C4Diagram({
  view,
  nodesById,
  selectedNodeId,
  selectedRelationship,
  transform,
  onSelectNode,
  onSelectRelationship
}: {
  view: View;
  nodesById: Map<Id, ArchNode>;
  selectedNodeId: Id | null;
  selectedRelationship: Extract<Selection, { kind: "relationship" }> | null;
  transform: DiagramTransform;
  onSelectNode: (id: Id) => void;
  onSelectRelationship: (relationship: Relationship) => void;
}) {
  const nodeWidth = 156;
  const nodeHeight = 62;
  const laneWidth = 210;
  const marginX = 56;
  const marginY = 76;
  const rowGap = 86;
  const allNodeIds = view.lanes.flatMap((lane) => lane.nodeIds);
  const visibleNodeIds = new Set(allNodeIds);
  const canvasWidth = Math.max(760, marginX * 2 + view.lanes.length * laneWidth + 40);
  const canvasHeight = Math.max(440, marginY + Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1) * rowGap + 88);
  const positionFor = (nodeId: Id) => {
    const laneIndex = view.lanes.findIndex((lane) => lane.nodeIds.includes(nodeId));
    const rowIndex = view.lanes[Math.max(laneIndex, 0)]?.nodeIds.indexOf(nodeId) ?? 0;
    return {
      x: marginX + Math.max(laneIndex, 0) * laneWidth,
      y: marginY + Math.max(rowIndex, 0) * rowGap
    };
  };
  const centerFor = (nodeId: Id) => {
    const position = positionFor(nodeId);
    return { x: position.x + nodeWidth / 2, y: position.y + nodeHeight / 2 };
  };
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
          relationshipType: "structural" as const
        };
      });
  });

  const pathFor = (relationship: Relationship, index: number) => {
    const from = centerFor(relationship.from);
    const to = centerFor(relationship.to);
    const direction = to.x >= from.x ? 1 : -1;
    const offset = (index % 3 - 1) * 18;
    const startX = from.x + direction * (nodeWidth / 2);
    const endX = to.x - direction * (nodeWidth / 2);
    const startY = from.y + offset;
    const endY = to.y + offset;
    const midX = (startX + endX) / 2;
    return {
      d: `M ${startX} ${startY} C ${midX} ${startY}, ${midX} ${endY}, ${endX} ${endY}`,
      labelX: midX,
      labelY: Math.min(startY, endY) - 10
    };
  };

  return (
    <section className="map-shell c4-shell">
      <div
        className={`c4-canvas ${view.type}`}
        style={{ width: canvasWidth, height: canvasHeight, transform: `scale(${transform.zoom})`, transformOrigin: "0 0" }}
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
          {relationships.map((relationship, index) => {
            const route = pathFor(relationship, index);
            const selected = selectedRelationship?.from === relationship.from && selectedRelationship.to === relationship.to;
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
                  className="c4-relationship"
                  d={route.d}
                  markerEnd={selected ? "url(#c4-arrowhead-selected)" : "url(#c4-arrowhead)"}
                />
                <text className="relationship-label c4-label" x={route.labelX} y={route.labelY}>{relationship.label}</text>
              </g>
            );
          })}
        </svg>
        <div className="c4-boundary">
          <span>{view.type === "c4-context" ? "System boundary" : view.type === "c4-container" ? "Container boundary" : "Component scope"}</span>
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
              style={{ left: position.x, top: position.y, width: nodeWidth, minHeight: nodeHeight }}
              onClick={() => onSelectNode(node.id)}
              aria-label={`${node.name}, ${node.type}. ${node.summary}`}
            >
              <strong>{node.name}</strong>
              <span>{node.type}</span>
              <small>{node.summary}</small>
            </button>
          );
        })}
      </div>
      <div className="edge-strip">
        <span className="edge-count">{relationships.length} labeled structural relationships</span>
      </div>
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

  return (
    <section className="map-shell sequence-shell">
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
        style={{ transform: `scale(${transform.zoom})`, transformOrigin: "0 0" }}
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
        {activeFlow.steps.map((step, index) => {
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
              <rect className="sequence-step-dot" x={midX - 10} y={y - 10} width="20" height="20" />
              <text className="sequence-step-label" x={midX} y={y + 4}>{index + 1}</text>
              <text className="sequence-action" x={midX} y={y - 17}>{step.action.length > 26 ? `${step.action.slice(0, 23)}...` : step.action}</text>
              <text className="sequence-data" x={midX} y={y + 30}>{dataLabel}</text>
            </g>
          );
        })}
      </svg>
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
