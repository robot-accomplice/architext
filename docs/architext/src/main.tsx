import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
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
  | { kind: "step"; flowId: Id; stepId: Id };

const statusLabels: Record<Flow["status"], string> = {
  implemented: "Implemented",
  partial: "Partial",
  planned: "Planned"
};

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

function FieldList({ title, items }: { title: string; items: string[] }) {
  return (
    <section className="detail-section">
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
  const [navCollapsed, setNavCollapsed] = useState(false);
  const [query, setQuery] = useState("");
  const [activeViewId, setActiveViewId] = useState<Id>("");
  const [activeFlowId, setActiveFlowId] = useState<Id>("");
  const [selection, setSelection] = useState<Selection | null>(null);

  useEffect(() => {
    loadModel()
      .then((loaded) => {
        setModel(loaded);
        setActiveViewId(loaded.manifest.defaultViewId);
        setActiveFlowId(loaded.flows[0]?.id ?? "");
        setSelection({ kind: "flow", id: loaded.flows[0]?.id ?? "" });
      })
      .catch((loadError: unknown) => {
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      });
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
  const activeView = viewsById.get(activeViewId) ?? model.views[0];
  const flowNodeIds = new Set(activeFlow.steps.flatMap((step) => [step.from, step.to]));
  const selectedNodeId = selection?.kind === "node" ? selection.id : null;
  const selectedStep = selection?.kind === "step"
    ? flowsById.get(selection.flowId)?.steps.find((step) => step.id === selection.stepId)
    : null;

  const filteredFlows = model.flows.filter((flow) => {
    const text = [flow.name, flow.summary, flow.status, flow.trigger, ...flow.knownGaps].join(" ").toLowerCase();
    return text.includes(query.toLowerCase());
  });

  return (
    <div className={`app ${navCollapsed ? "nav-collapsed" : ""}`}>
      <header className="topbar">
        <div>
          <p className="eyebrow">Architext / {model.manifest.schemaVersion}</p>
          <h1>{model.manifest.project.name}</h1>
          <p>{model.manifest.project.summary}</p>
        </div>
        <div className="topbar-actions">
          <select value={activeViewId} onChange={(event) => setActiveViewId(event.target.value)} aria-label="Select view">
            {model.views.map((view) => (
              <option key={view.id} value={view.id}>{view.name}</option>
            ))}
          </select>
          <button type="button" onClick={() => setNavCollapsed((value) => !value)}>
            {navCollapsed ? "Show nav" : "Collapse nav"}
          </button>
        </div>
      </header>

      <aside className="left-nav">
        <div className="panel-head">
          <h2>Flows</h2>
          <input
            type="search"
            value={query}
            placeholder="Search flows"
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        <div className="flow-list">
          {filteredFlows.map((flow) => (
            <button
              key={flow.id}
              type="button"
              className={`flow-card ${flow.id === activeFlow.id ? "active" : ""}`}
              onClick={() => {
                setActiveFlowId(flow.id);
                setSelection({ kind: "flow", id: flow.id });
              }}
            >
              <strong>{flow.name}</strong>
              <span>{flow.summary}</span>
              <Badge tone={flow.status}>{statusLabels[flow.status]}</Badge>
            </button>
          ))}
        </div>
      </aside>

      <main className="diagram-area">
        <section className="diagram-header">
          <div>
            <h2>{activeView.name}</h2>
            <p>{activeView.summary}</p>
          </div>
          <div className="legend">
            {(["actor", "client", "service", "worker", "queue", "data-store", "external-service"] as NodeType[]).map((type) => (
              <span key={type}><i className={`dot ${type}`} />{type}</span>
            ))}
          </div>
        </section>

        <SystemMap
          view={activeView}
          nodesById={nodesById}
          activeFlow={activeFlow}
          selectedNodeId={selectedNodeId}
          onSelectNode={(id) => setSelection({ kind: "node", id })}
        />

        <section className="steps">
          <div className="steps-head">
            <h2>{activeFlow.name}</h2>
            <Badge tone={activeFlow.status}>{statusLabels[activeFlow.status]}</Badge>
          </div>
          <p>{activeFlow.summary}</p>
          <div className="step-list">
            {activeFlow.steps.map((step, index) => (
              <button
                key={step.id}
                type="button"
                className={`step-card ${selection?.kind === "step" && selection.stepId === step.id ? "active" : ""}`}
                onClick={() => setSelection({ kind: "step", flowId: activeFlow.id, stepId: step.id })}
              >
                <span className="step-number">{index + 1}</span>
                <strong>{nodesById.get(step.from)?.name ?? step.from} {"->"} {nodesById.get(step.to)?.name ?? step.to}</strong>
                <span>{step.action}</span>
              </button>
            ))}
          </div>
        </section>
      </main>

      <aside className="details">
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
      </aside>
    </div>
  );
}

function SystemMap({
  view,
  nodesById,
  activeFlow,
  selectedNodeId,
  onSelectNode
}: {
  view: View;
  nodesById: Map<Id, ArchNode>;
  activeFlow: Flow;
  selectedNodeId: Id | null;
  onSelectNode: (id: Id) => void;
}) {
  const flowNodeIds = new Set(activeFlow.steps.flatMap((step) => [step.from, step.to]));
  const edgePairs = new Set(activeFlow.steps.map((step) => `${step.from}->${step.to}`));

  return (
    <section className="map-shell">
      <div className="lane-grid" style={{ gridTemplateColumns: `repeat(${view.lanes.length}, minmax(180px, 1fr))` }}>
        {view.lanes.map((lane) => (
          <div className="lane" key={lane.id}>
            <h3>{lane.name}</h3>
            <div className="lane-nodes">
              {lane.nodeIds.map((nodeId) => {
                const node = nodesById.get(nodeId);
                if (!node) return null;
                const isActive = flowNodeIds.has(node.id);
                const isSelected = selectedNodeId === node.id;
                return (
                  <button
                    key={node.id}
                    type="button"
                    className={`node-card ${node.type} ${isActive ? "in-flow" : ""} ${isSelected ? "selected" : ""}`}
                    onClick={() => onSelectNode(node.id)}
                  >
                    <strong>{node.name}</strong>
                    <span>{node.type}</span>
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>
      <div className="edge-strip">
        {activeFlow.steps.map((step, index) => (
          <span className="edge-chip" key={step.id} title={step.summary}>
            {index + 1}. {step.from} {"->"} {step.to}
          </span>
        ))}
        <span className="edge-count">{edgePairs.size} highlighted transitions</span>
      </div>
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
  if (selection?.kind === "node") {
    const node = nodesById.get(selection.id);
    if (!node) return <EmptyDetail />;
    const relatedFlows = node.relatedFlows.map((id) => flowsById.get(id)).filter(Boolean) as Flow[];
    const decisions = node.relatedDecisions.map((id) => decisionsById.get(id)).filter(Boolean) as Decision[];
    const risks = node.knownRisks.map((id) => risksById.get(id)).filter(Boolean) as Risk[];
    const dataClasses = node.dataHandled.map((id) => dataById.get(id)).filter(Boolean) as DataClass[];

    return (
      <div className="detail-content">
        <p className="eyebrow">{node.type}</p>
        <h2>{node.name}</h2>
        <p>{node.summary}</p>
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
      </div>
    );
  }

  if (selection?.kind === "step" && selectedStep) {
    const from = nodesById.get(selectedStep.from);
    const to = nodesById.get(selectedStep.to);
    const dataClasses = selectedStep.data.map((id) => dataById.get(id)).filter(Boolean) as DataClass[];
    return (
      <div className="detail-content">
        <p className="eyebrow">Flow step</p>
        <h2>{selectedStep.action}</h2>
        <p>{selectedStep.summary}</p>
        <div className="path-pair">
          <button type="button" onClick={() => onSelectNode(selectedStep.from)}>{from?.name ?? selectedStep.from}</button>
          <span>{"->"}</span>
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
      </div>
    );
  }

  const flow = selection?.kind === "flow" ? flowsById.get(selection.id) ?? activeFlow : activeFlow;
  return (
    <div className="detail-content">
      <p className="eyebrow">Flow</p>
      <h2>{flow.name}</h2>
      <p>{flow.summary}</p>
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
    <section className="detail-section">
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
    <section className="detail-section">
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

createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
