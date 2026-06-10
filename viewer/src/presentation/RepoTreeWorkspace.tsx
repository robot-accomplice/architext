import React, { useMemo, useState } from "react";
import { buildRepoTree, buildOwnerIndex, resolveOwner, dominantOwner } from "./repoTreeModel.js";
import { buildFlowColorMap, colorForOwner } from "./repoTreeColors.js";
import { fileTypeLabel, formatSize, formatRelativeTime } from "./repoTreeFormat.js";
import { DiagramIcon } from "./DiagramIcon.js";
import type { ArchNode, Flow, Id } from "../domain/architectureTypes.js";

type Lens = "c4" | "flow";
type TreeNode = {
  name: string;
  path: string;
  type: "dir" | "file";
  size?: number | null;
  mtime?: number | null;
  children?: TreeNode[];
};

export function RepoTreeWorkspace({ files, source, nodes, flows, lens, onSelectNode }: {
  files: Array<{ path: string; size: number | null; mtime: number | null }>;
  source?: string;
  nodes: ArchNode[];
  flows: Flow[];
  lens: Lens;
  onSelectNode: (id: Id) => void;
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  const tree = useMemo(() => buildRepoTree(files) as TreeNode, [files]);
  const ownerIndex = useMemo(() => buildOwnerIndex(nodes), [nodes]);
  const flowColorMap = useMemo(() => buildFlowColorMap(flows), [flows]);
  const now = useMemo(() => Date.now(), [files]);

  const ownerColor = (owner: ArchNode | null) => colorForOwner(owner, lens, flowColorMap);
  const ownerLabel = (owner: ArchNode | null) => owner?.name ?? owner?.id ?? "";

  const toggle = (path: string) => setCollapsed((prev) => {
    const next = new Set(prev);
    if (next.has(path)) next.delete(path); else next.add(path);
    return next;
  });

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => {
    const indent = depth * 14;

    if (node.type === "file") {
      const owner = resolveOwner(node.path, ownerIndex);
      const color = ownerColor(owner);
      const typeLabel = fileTypeLabel(node.name);
      return (
        <div
          key={node.path}
          className={`repo-tree-row file${owner ? " owned" : ""}`}
          style={{ borderLeftColor: color ?? "transparent" }}
          title={owner ? `${ownerLabel(owner)} (${owner.type})` : "Not mapped to an architecture node"}
          onClick={owner ? () => onSelectNode(owner.id) : undefined}
          role={owner ? "button" : undefined}
          tabIndex={owner ? 0 : undefined}
        >
          <span className="repo-indent" style={{ width: indent }} />
          <span className="repo-caret-slot" />
          {typeLabel
            ? <span className="repo-badge">{typeLabel}</span>
            : <span className="repo-icon"><DiagramIcon icon="file" className="repo-glyph" /></span>}
          <span className="repo-name">{node.name}</span>
          <span className="repo-meta repo-size">{formatSize(node.size ?? null)}</span>
          <span className="repo-meta repo-time">{formatRelativeTime(node.mtime ?? null, now)}</span>
          {owner ? <span className="repo-owner" style={{ color: color ?? undefined }}>{ownerLabel(owner)}</span> : <span className="repo-owner" />}
        </div>
      );
    }

    const isCollapsed = collapsed.has(node.path);
    const { owner, mixed } = dominantOwner(node, ownerIndex);
    const color = mixed ? null : ownerColor(owner);
    return (
      <div key={node.path || "root"}>
        {node.path ? (
          <div
            className="repo-tree-row dir"
            style={{ borderLeftColor: color ?? "transparent" }}
            onClick={() => toggle(node.path)}
            role="button"
            tabIndex={0}
          >
            <span className="repo-indent" style={{ width: indent }} />
            <span className={`repo-caret${isCollapsed ? " collapsed" : ""}`} aria-hidden="true">▾</span>
            <span className="repo-icon">
              <DiagramIcon icon={isCollapsed ? "folder" : "folder-open"} className="repo-glyph" />
            </span>
            <span className="repo-name dir">{node.name}</span>
            <span className="repo-meta repo-size" />
            <span className="repo-meta repo-time" />
            {mixed
              ? <span className="repo-owner muted">mixed</span>
              : owner ? <span className="repo-owner" style={{ color: color ?? undefined }}>{ownerLabel(owner)}</span> : <span className="repo-owner" />}
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
      </div>
      <div className="repo-tree-body">
        <div className="repo-tree-inner">
          {files.length ? renderNode(tree, 0) : (
            <p className="repo-tree-empty">No files found. Run <code>architext serve</code> inside a git repository.</p>
          )}
        </div>
      </div>
    </div>
  );
}
