import React from "react";
import { DiagramIcon } from "./DiagramIcon.js";
import { iconForNodeType } from "./diagramIconModel.js";
import { formatSize } from "./repoTreeFormat.js";
import { C4_COLOR } from "./repoTreeColors.js";
import type { Id } from "../domain/architectureTypes.js";

type NodeRef = { id: Id; name: string; type: string };

const nodeColor = (type: string) => (C4_COLOR as Record<string, string>)[type] ?? "var(--dim)";

// A blast-radius section: a counted heading plus its body, hidden when empty.
// `accent` tints the card's left rail + count pill so the grid reads in color.
function Section({ title, count, accent, children }: { title: string; count: number; accent: string; children: React.ReactNode }) {
  if (!count) return null;
  return (
    <section className="blast-section" style={{ ["--card-accent" as string]: accent }}>
      <h3 className="blast-section-title">{title}<span className="blast-count">{count}</span></h3>
      {children}
    </section>
  );
}

// Node chips tinted by the component's C4 type — same color language as the C4
// diagrams and the Repo Tree, so a "service" reads the same color everywhere.
function NodeChips({ nodes, onFocusNode }: { nodes: NodeRef[]; onFocusNode: (id: Id) => void }) {
  return (
    <div className="blast-chips">
      {nodes.map((n) => (
        <button
          key={n.id}
          type="button"
          className="blast-chip node"
          style={{ borderColor: nodeColor(n.type), color: nodeColor(n.type) }}
          onClick={() => onFocusNode(n.id)}
          title={`${n.name} (${n.type})`}
        >
          <DiagramIcon icon={iconForNodeType(n.type)} className="blast-chip-icon" />
          <span>{n.name}</span>
        </button>
      ))}
    </div>
  );
}

export function BlastRadiusWorkspace({ radius, hasQuery, onFocusNode, onSelectFlow, onSelectView }: {
  radius: any | null;
  hasQuery: boolean;
  onFocusNode: (id: Id) => void;
  onSelectFlow: (id: Id) => void;
  onSelectView: (id: Id) => void;
}) {
  if (!radius) {
    return (
      <div className="blast-workspace">
        <div className="blast-empty">
          <h2>Blast radius</h2>
          <p>
            {hasQuery
              ? "No component selected. Pick a result on the left to see everything it reaches."
              : "Search for a component, file, or concept on the left to see where it reaches across the repository and architecture — its files, dependencies, dependents, flows, decisions, and risks."}
          </p>
        </div>
      </div>
    );
  }

  const r = radius;
  const reachCount =
    r.dependsOn.length + r.dependents.length + r.flows.length + r.decisions.length +
    r.risks.length + r.dataHandled.length + r.views.length + r.ownedFiles.length;

  return (
    <div className="blast-workspace">
      <header className="blast-head" style={{ ["--node-accent" as string]: nodeColor(r.node.type) }}>
        <p className="blast-eyebrow">{r.node.type}</p>
        <h2>{r.node.name}</h2>
        <p className="blast-reach">Reaches {reachCount} element{reachCount === 1 ? "" : "s"} across the repository.</p>
      </header>

      <div className="blast-sections">
      <Section title="Owns files" count={r.ownedFiles.length} accent="var(--green)">
        <ul className="blast-files">
          {r.ownedFiles.map((f: { path: string; size: number | null }) => (
            <li key={f.path}><span className="blast-file-path">{f.path}</span><span className="blast-file-size">{formatSize(f.size)}</span></li>
          ))}
        </ul>
      </Section>

      <Section title="Depends on" count={r.dependsOn.length} accent="var(--blue)">
        <NodeChips nodes={r.dependsOn} onFocusNode={onFocusNode} />
      </Section>

      <Section title="Depended on by" count={r.dependents.length} accent="var(--purple)">
        <NodeChips nodes={r.dependents} onFocusNode={onFocusNode} />
      </Section>

      <Section title="Flows" count={r.flows.length} accent="var(--orange)">
        <div className="blast-chips">
          {r.flows.map((f: { id: Id; name: string }) => (
            <button key={f.id} type="button" className="blast-chip flow" onClick={() => onSelectFlow(f.id)}>{f.name}</button>
          ))}
        </div>
      </Section>

      <Section title="Appears in views" count={r.views.length} accent="var(--cyan)">
        <div className="blast-chips">
          {r.views.map((v: { id: Id; name: string }) => (
            <button key={v.id} type="button" className="blast-chip view" onClick={() => onSelectView(v.id)}>{v.name}</button>
          ))}
        </div>
      </Section>

      <Section title="Data handled" count={r.dataHandled.length} accent="var(--pink)">
        <div className="blast-chips">
          {r.dataHandled.map((d: { id: Id; name: string; sensitivity?: string }) => (
            <span key={d.id} className={`blast-chip data sensitivity-${d.sensitivity ?? "low"}`}>{d.name}</span>
          ))}
        </div>
      </Section>

      <Section title="Decisions" count={r.decisions.length} accent="var(--yellow)">
        <ul className="blast-list">
          {r.decisions.map((d: { id: Id; title: string }) => <li key={d.id}>{d.title}</li>)}
        </ul>
      </Section>

      <Section title="Risks" count={r.risks.length} accent="var(--red)">
        <ul className="blast-list">
          {r.risks.map((rk: { id: Id; title: string; severity?: string }) => (
            <li key={rk.id}><span className={`blast-sev sensitivity-${rk.severity ?? "low"}`}>{rk.severity ?? "?"}</span>{rk.title}</li>
          ))}
        </ul>
      </Section>
      </div>
    </div>
  );
}
