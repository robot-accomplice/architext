import React, { useMemo, useState } from "react";
import { buildRepoTree, buildOwnerIndex, resolveOwner, dominantOwner } from "./repoTreeModel.js";
import { buildFlowColorMap, colorForOwner } from "./repoTreeColors.js";
import { fileIconSpec, formatSize, formatRelativeTime } from "./repoTreeFormat.js";
import { DiagramIcon } from "./DiagramIcon.js";
import type { ArchNode, Flow, Id } from "../domain/architectureTypes.js";

// Bundled technology brand logos (Devicon, vendored). Vite inlines the URLs at
// build time so the viewer needs no network to render file-type icons.
const BRAND_ICONS = import.meta.glob("../assets/file-icons/*.svg", {
  eager: true,
  query: "?url",
  import: "default"
}) as Record<string, string>;
const brandUrlByKey = new Map<string, string>(
  Object.entries(BRAND_ICONS).map(([path, url]) => [path.replace(/.*\/([^/]+)\.svg$/, "$1"), url])
);

function FileIcon({ name }: { name: string }) {
  const spec = fileIconSpec(name);
  if (spec.kind === "brand") {
    const url = brandUrlByKey.get(spec.key);
    if (url) return <img className="repo-file-img" src={url} alt="" aria-hidden="true" />;
  }
  const glyph = spec.kind === "glyph" ? spec : { icon: "code", color: undefined };
  return (
    <span className="repo-icon" style={{ color: glyph.color }}>
      <DiagramIcon icon={glyph.icon} className="repo-glyph" />
    </span>
  );
}

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
          <FileIcon name={node.name} />
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
          {files.length ? (
            <>
              <div className="repo-tree-colhead" aria-hidden="true">
                <span className="repo-colhead-name">Name</span>
                <span className="repo-meta repo-size">Size</span>
                <span className="repo-meta repo-time">Modified</span>
                <span className="repo-owner repo-colhead-owner">Owner</span>
              </div>
              {renderNode(tree, 0)}
            </>
          ) : (
            <p className="repo-tree-empty">No files found. Run <code>architext serve</code> inside a git repository.</p>
          )}
        </div>
      </div>
    </div>
  );
}
