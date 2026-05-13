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
  const [navCollapsed, setNavCollapsed] = useState(() => localStorage.getItem("architext-left-collapsed") === "true");
  const [rightCollapsed, setRightCollapsed] = useState(() => localStorage.getItem("architext-right-collapsed") === "true");
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

  useEffect(() => {
    localStorage.setItem("architext-left-collapsed", String(navCollapsed));
  }, [navCollapsed]);

  useEffect(() => {
    localStorage.setItem("architext-right-collapsed", String(rightCollapsed));
  }, [rightCollapsed]);

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
  const isC4View = activeView.type.startsWith("c4-");
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
    <div className={`app ${navCollapsed ? "left-collapsed" : ""} ${rightCollapsed ? "right-collapsed" : ""}`}>
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
          <div className="panel-rail">Flows</div>
        ) : (
          <>
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
          </>
        )}
      </aside>

      <main className="diagram-area">
        <section className="diagram-header">
          <div>
            <h2>{activeView.name}</h2>
            <p>{activeView.summary}</p>
          </div>
          <div className="legend">
            {(["actor", "software-system", "client", "service", "worker", "queue", "data-store", "external-service"] as NodeType[]).map((type) => (
              <span key={type}><i className={`dot ${type}`} />{type}</span>
            ))}
          </div>
        </section>

        {activeView.type === "sequence" ? (
          <SequenceDiagram
            activeFlow={activeFlow}
            nodesById={nodesById}
            dataById={dataById}
            selectedStepId={selectedStep?.id ?? null}
            onSelectStep={(stepId) => setSelection({ kind: "step", flowId: activeFlow.id, stepId })}
          />
        ) : (
          <SystemMap
            view={activeView}
            nodesById={nodesById}
            activeFlow={isC4View ? null : activeFlow}
            showStructuralConnections={isC4View}
            selectedStepId={selectedStep?.id ?? null}
            selectedNodeId={selectedNodeId}
            onSelectNode={(id) => setSelection({ kind: "node", id })}
          />
        )}

        {!isC4View && (
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

function SystemMap({
  view,
  nodesById,
  activeFlow,
  showStructuralConnections,
  selectedStepId,
  selectedNodeId,
  onSelectNode
}: {
  view: View;
  nodesById: Map<Id, ArchNode>;
  activeFlow: Flow | null;
  showStructuralConnections: boolean;
  selectedStepId: Id | null;
  selectedNodeId: Id | null;
  onSelectNode: (id: Id) => void;
}) {
  const visibleNodeIds = new Set(view.lanes.flatMap((lane) => lane.nodeIds));
  const flowNodeIds = new Set(activeFlow ? activeFlow.steps.flatMap((step) => [step.from, step.to]) : Array.from(visibleNodeIds));
  const nodeWidth = 150;
  const nodeHeight = 48;
  const laneWidth = 188;
  const rowGap = 66;
  const marginX = 58;
  const marginY = 62;
  const laneIndexByNode = new Map<Id, number>();
  const rowIndexByNode = new Map<Id, number>();

  view.lanes.forEach((lane, laneIndex) => {
    lane.nodeIds.forEach((nodeId, rowIndex) => {
      laneIndexByNode.set(nodeId, laneIndex);
      rowIndexByNode.set(nodeId, rowIndex);
    });
  });

  const laneHeight = Math.max(...view.lanes.map((lane) => lane.nodeIds.length), 1) * rowGap + marginY + 24;
  const canvasWidth = marginX * 2 + view.lanes.length * laneWidth;
  const canvasHeight = Math.max(360, laneHeight);
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
  type Route = { d: string; labelX: number; labelY: number; cost: number };

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

  const routeCost = (start: Point, controlA: Point, controlB: Point, end: Point, fromId: Id, toId: Id) => {
    const blockers = Array.from(visibleNodeIds)
      .filter((nodeId) => nodeId !== fromId && nodeId !== toId)
      .map(rectFor);
    let cost = Math.hypot(end.x - start.x, end.y - start.y);
    for (let step = 1; step < 32; step += 1) {
      const point = cubicPoint(start, controlA, controlB, end, step / 32);
      for (const rect of blockers) {
        const padding = 8;
        const inside =
          point.x >= rect.x - padding &&
          point.x <= rect.x + rect.width + padding &&
          point.y >= rect.y - padding &&
          point.y <= rect.y + rect.height + padding;
        if (inside) cost += 900;
      }
    }
    return cost;
  };

  const cubicRoute = (
    fromId: Id,
    toId: Id,
    startSide: Side,
    endSide: Side,
    controlA: Point,
    controlB: Point,
    label: Point
  ): Route => {
    const start = anchorFor(rectFor(fromId), startSide);
    const end = anchorFor(rectFor(toId), endSide);
    return {
      d: `M ${start.x} ${start.y} C ${controlA.x} ${controlA.y}, ${controlB.x} ${controlB.y}, ${end.x} ${end.y}`,
      labelX: label.x,
      labelY: label.y,
      cost: routeCost(start, controlA, controlB, end, fromId, toId)
    };
  };

  const edgePath = (fromId: Id, toId: Id, index: number) => {
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

    if (fromLane === toLane) {
      const leftGutter = Math.min(fromRect.x, toRect.x) - 28 - (index % 2) * 14;
      const rightGutter = Math.max(fromRect.x + nodeWidth, toRect.x + nodeWidth) + 28 + (index % 2) * 14;
      candidates.push(
        cubicRoute(
          fromId,
          toId,
          "left",
          "left",
          { x: leftGutter, y: fromCenter.y },
          { x: leftGutter, y: toCenter.y },
          { x: leftGutter, y: mid.y }
        ),
        cubicRoute(
          fromId,
          toId,
          "right",
          "right",
          { x: rightGutter, y: fromCenter.y },
          { x: rightGutter, y: toCenter.y },
          { x: rightGutter, y: mid.y }
        )
      );
    }

    if (toLane > fromLane) {
      const bend = Math.max(38, Math.abs(to.x - (from.x + nodeWidth)) * 0.42);
      candidates.push(cubicRoute(
        fromId,
        toId,
        "right",
        "left",
        { x: from.x + nodeWidth + bend, y: fromCenter.y },
        { x: to.x - bend, y: toCenter.y },
        mid
      ));
    }

    if (toLane < fromLane) {
      const bend = Math.max(38, Math.abs(from.x - (to.x + nodeWidth)) * 0.42);
      candidates.push(cubicRoute(
        fromId,
        toId,
        "left",
        "right",
        { x: from.x - bend, y: fromCenter.y },
        { x: to.x + nodeWidth + bend, y: toCenter.y },
        mid
      ));
    }

    const topCorridor = Math.max(20, Math.min(fromRect.y, toRect.y) - 34 - (index % 2) * 16);
    const bottomCorridor = Math.max(fromRect.y + nodeHeight, toRect.y + nodeHeight) + 34 + (index % 2) * 16;
    candidates.push(
      cubicRoute(
        fromId,
        toId,
        "top",
        "top",
        { x: fromCenter.x, y: topCorridor },
        { x: toCenter.x, y: topCorridor },
        { x: mid.x, y: topCorridor }
      ),
      cubicRoute(
        fromId,
        toId,
        "bottom",
        "bottom",
        { x: fromCenter.x, y: bottomCorridor },
        { x: toCenter.x, y: bottomCorridor },
        { x: mid.x, y: bottomCorridor }
      )
    );

    return candidates.sort((a, b) => a.cost - b.cost)[0];
  };

  return (
    <section className="map-shell">
      <div className="diagram-canvas" style={{ width: canvasWidth, height: canvasHeight }}>
        <svg className="flow-lines" width={canvasWidth} height={canvasHeight} aria-hidden="true">
          <defs>
            <marker id="arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
            <marker id="arrowhead-selected" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
          </defs>
          {showStructuralConnections && Array.from(visibleNodeIds).flatMap((nodeId) => {
            const node = nodesById.get(nodeId);
            return (node?.dependencies ?? [])
              .filter((dependencyId) => visibleNodeIds.has(dependencyId))
              .map((dependencyId) => ({ from: nodeId, to: dependencyId }));
          }).map((connection, index) => {
            const route = edgePath(connection.from, connection.to, index);
            return (
              <path
                key={`${connection.from}-${connection.to}`}
                className="structural-line"
                d={route.d}
                markerEnd="url(#arrowhead)"
              />
            );
          })}
          {!showStructuralConnections && activeFlow?.steps.map((step, index) => {
            if (!laneIndexByNode.has(step.from) || !laneIndexByNode.has(step.to)) {
              return null;
            }
            const route = edgePath(step.from, step.to, index);
            const isSelected = selectedStepId === step.id;
            return (
              <g key={step.id} className={isSelected ? "flow-edge selected" : "flow-edge"}>
                <path
                  className="flow-line"
                  d={route.d}
                  markerEnd={isSelected ? "url(#arrowhead-selected)" : "url(#arrowhead)"}
                />
                <circle className="flow-step-dot" cx={route.labelX} cy={route.labelY} r="13" />
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
          {activeFlow.steps.map((step, index) => (
            <span className="edge-chip" key={step.id} title={step.summary}>
              {index + 1}. {step.from} {"->"} {step.to}
            </span>
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

function SequenceDiagram({
  activeFlow,
  nodesById,
  dataById,
  selectedStepId,
  onSelectStep
}: {
  activeFlow: Flow;
  nodesById: Map<Id, ArchNode>;
  dataById: Map<Id, DataClass>;
  selectedStepId: Id | null;
  onSelectStep: (stepId: Id) => void;
}) {
  const participantIds = Array.from(new Set(activeFlow.steps.flatMap((step) => [step.from, step.to])));
  const participantWidth = 158;
  const rowHeight = 70;
  const marginX = 34;
  const headerY = 26;
  const messageStartY = 112;
  const width = marginX * 2 + participantIds.length * participantWidth;
  const height = messageStartY + activeFlow.steps.length * rowHeight + 42;
  const xFor = (id: Id) => marginX + participantIds.indexOf(id) * participantWidth + participantWidth / 2;

  return (
    <section className="map-shell sequence-shell">
      <svg className="sequence-canvas" width={width} height={height} role="img" aria-label={`${activeFlow.name} sequence diagram`}>
        <defs>
          <marker id="sequence-arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
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
              <rect className={`sequence-participant ${node?.type ?? ""}`} x={x - 64} y={headerY} width="128" height="46" rx="6" />
              <text className="sequence-title" x={x} y={headerY + 20}>{node?.name ?? id}</text>
              <text className="sequence-kind" x={x} y={headerY + 35}>{node?.type ?? "node"}</text>
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
          return (
            <g
              key={step.id}
              className={selectedStepId === step.id ? "sequence-message selected" : "sequence-message"}
              onClick={() => onSelectStep(step.id)}
            >
              <line
                className="sequence-line"
                x1={fromX}
                y1={y}
                x2={toX}
                y2={y}
                markerEnd={selectedStepId === step.id ? "url(#sequence-arrowhead-selected)" : "url(#sequence-arrowhead)"}
              />
              <circle className="sequence-step-dot" cx={midX} cy={y} r="13" />
              <text className="sequence-step-label" x={midX} y={y + 4}>{index + 1}</text>
              <text className="sequence-action" x={midX} y={y - 17}>{step.action}</text>
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
