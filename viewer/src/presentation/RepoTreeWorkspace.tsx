import React, { useMemo, useState } from "react";
import { buildRepoTree, buildOwnerIndex, resolveOwner, dominantOwner } from "./repoTreeModel.js";
import type { ArchNode, Flow, Id, NodeType } from "../domain/architectureTypes.js";

// C4 lens: owning node type -> the existing diagram colour palette.
const C4_COLOR: Partial<Record<NodeType, string>> = {
  actor: "var(--pink)",
  "software-system": "var(--cyan)",
  client: "var(--blue)",
  service: "var(--purple)",
  worker: "var(--purple)",
  queue: "var(--orange)",
  "data-store": "var(--green)",
  "external-service": "var(--muted)",
  module: "var(--c4-module)",
  "deployment-unit": "var(--c4-deployment)"
};
// Flow lens: stable per-flow palette assigned by flow order.
const FLOW_PALETTE = ["var(--blue)", "var(--green)", "var(--orange)", "var(--pink)", "var(--purple)", "var(--cyan)", "var(--yellow)", "var(--red)"];

type Lens = "c4" | "flow";
type TreeNode = { name: string; path: string; type: "dir" | "file"; children?: TreeNode[] };

export function RepoTreeWorkspace({ files, source, nodes, flows, onSelectNode }: {
  files: string[];
  source?: string;
  nodes: ArchNode[];
  flows: Flow[];
  onSelectNode: (id: Id) => void;
}) {
  const [lens, setLens] = useState<Lens>("c4");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  const tree = useMemo(() => buildRepoTree(files) as TreeNode, [files]);
  const ownerIndex = useMemo(() => buildOwnerIndex(nodes), [nodes]);
  const flowColor = useMemo(() => {
    const map = new Map<Id, string>();
    (flows ?? []).forEach((flow, index) => map.set(flow.id, FLOW_PALETTE[index % FLOW_PALETTE.length]));
    return map;
  }, [flows]);

  const colorForOwner = (owner: ArchNode | null): string | null => {
    if (!owner) return null;
    if (lens === "c4") return C4_COLOR[owner.type] ?? "var(--dim)";
    const flowId = owner.relatedFlows?.[0];
    return flowId ? flowColor.get(flowId) ?? "var(--dim)" : null;
  };

  const toggle = (path: string) => setCollapsed((prev) => {
    const next = new Set(prev);
    if (next.has(path)) next.delete(path); else next.add(path);
    return next;
  });

  const ownerLabel = (owner: ArchNode | null) => owner?.name ?? owner?.id ?? "";

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => {
    if (node.type === "file") {
      const owner = resolveOwner(node.path, ownerIndex);
      const color = colorForOwner(owner);
      return (
        <div
          key={node.path}
          className={`repo-tree-row file${owner ? " owned" : ""}`}
          style={{ paddingLeft: depth * 14 + 10 }}
          title={owner ? `${ownerLabel(owner)} (${owner.type})` : "Not mapped to an architecture node"}
          onClick={owner ? () => onSelectNode(owner.id) : undefined}
          role={owner ? "button" : undefined}
          tabIndex={owner ? 0 : undefined}
        >
          <span className="repo-swatch" style={{ background: color ?? "transparent", borderColor: color ?? "var(--line)" }} />
          <span className="repo-name">{node.name}</span>
          {owner ? <span className="repo-owner">{ownerLabel(owner)}</span> : null}
        </div>
      );
    }

    const isCollapsed = collapsed.has(node.path);
    const { owner, mixed } = dominantOwner(node, ownerIndex);
    const color = colorForOwner(owner);
    return (
      <div key={node.path || "root"}>
        {node.path ? (
          <div className="repo-tree-row dir" style={{ paddingLeft: depth * 14 + 10 }} onClick={() => toggle(node.path)} role="button" tabIndex={0}>
            <span className={`repo-caret${isCollapsed ? " collapsed" : ""}`} aria-hidden="true">▾</span>
            <span className="repo-swatch dir" style={{ background: color ?? "transparent", borderColor: color ?? "var(--line)" }} />
            <span className="repo-name dir">{node.name}</span>
            {mixed ? <span className="repo-owner muted">mixed</span> : owner ? <span className="repo-owner">{ownerLabel(owner)}</span> : null}
          </div>
        ) : null}
        {!isCollapsed ? (node.children ?? []).map((child) => renderNode(child, node.path ? depth + 1 : 0)) : null}
      </div>
    );
  };

  return (
    <div className="repo-tree-workspace">
      <div className="repo-tree-header">
        <div className="panel-head">
          <h2>Repo Tree</h2>
          <p>{files.length} files{source ? ` · via ${source}` : ""} — colored by the architecture node that owns each path.</p>
        </div>
        <div className="repo-tree-lens" role="group" aria-label="Color lens">
          <button type="button" className={lens === "c4" ? "active" : ""} onClick={() => setLens("c4")}>C4 type</button>
          <button type="button" className={lens === "flow" ? "active" : ""} onClick={() => setLens("flow")}>Flow</button>
        </div>
      </div>
      <div className="repo-tree-body">
        {files.length ? renderNode(tree, 0) : (
          <p className="repo-tree-empty">No files found. Run <code>architext serve</code> inside a git repository.</p>
        )}
      </div>
    </div>
  );
}
