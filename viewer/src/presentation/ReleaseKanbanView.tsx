import type { Id, ReleaseDetail, ReleaseItem, Selection } from "../domain/architectureTypes.js";
import { Badge } from "./Badge.js";
import { releaseKanbanColumns } from "./releaseKanban.js";
import {
  activeReleaseBlockersForItem,
  blockersGroupedByItem,
  releaseBadgeTone,
  releaseLineCheckClass,
  releaseLineState,
  releaseScopeByItemId
} from "./releaseTruth.js";

function byId<T extends { id: Id }>(items: T[]): Map<Id, T> {
  return new Map(items.map((item) => [item.id, item]));
}

export function ReleaseKanban({
  detail,
  selection,
  onSelectItem
}: {
  detail: ReleaseDetail;
  selection: Selection | null;
  onSelectItem: (id: Id) => void;
}) {
  const columns = releaseKanbanColumns(detail) as { id: string; label: string; items: ReleaseItem[] }[];
  const blockersByItemId = blockersGroupedByItem(detail.blockers);
  const workstreamsById = byId(detail.workstreams);
  const scopeByItemId = releaseScopeByItemId(detail);

  return (
    <div className="release-kanban">
      {columns.map((column) => (
        <section className="release-kanban-column" key={column.id}>
          <header>
            <strong>{column.label}</strong>
            <span>{column.items.length}</span>
          </header>
          <div className="release-kanban-cards">
            {column.items.length ? column.items.map((item) => {
              const blockers = activeReleaseBlockersForItem(item, blockersByItemId.get(item.id) ?? []);
              const blocked = item.status === "blocked" || blockers.length > 0;
              const workstream = item.workstreamId ? workstreamsById.get(item.workstreamId) : undefined;
              return (
                <button
                  type="button"
                  className={`release-kanban-card stage-${column.id} ${selection?.kind === "release-item" && selection.itemId === item.id ? "active" : ""}`}
                  key={item.id}
                  onClick={() => onSelectItem(item.id)}
                >
                  <span className={`release-check ${releaseLineCheckClass(releaseLineState(item.status, blocked))}`} aria-label={releaseLineState(item.status, blocked)} />
                  <div>
                    <strong>{item.title}</strong>
                    <p>{item.summary}</p>
                    <small>{scopeByItemId.get(item.id) ?? "scope"} · {workstream?.name ?? "Unassigned"} · {item.kind}{item.priority ? ` · ${item.priority}` : ""}</small>
                  </div>
                  <div className="release-card-badges">
                    <Badge tone={releaseBadgeTone(blocked ? "blocked" : item.status)}>{releaseLineState(item.status, blocked)}</Badge>
                    {item.dependsOn?.length ? <Badge>{item.dependsOn.length} deps</Badge> : null}
                  </div>
                </button>
              );
            }) : (
              <p className="muted">No items.</p>
            )}
          </div>
        </section>
      ))}
    </div>
  );
}
