# Release Kanban Component

The Release Kanban view is Release Truth presentation code and should not live
inside `main.tsx`.

## Architecture

`presentation/ReleaseKanbanView.tsx` owns the Kanban projection for release items:
columns, item cards, blocked state display, workstream labels, scope labels, and
card selection. It consumes the existing release presentation helpers and the
shared `Badge` primitive.

`main.tsx` remains responsible for top-level mode and selection state. It passes
the active release detail, current selection, and item-selection callback into
the extracted component.

This is another incremental `main.tsx` decomposition slice. It does not change
Release Truth data, card ordering, or Kanban styling.

## Verification

- The extracted component keeps the same inputs and selection behavior.
- Shared badge rendering moves to `presentation/Badge.tsx`.
- The frontend build validates TypeScript imports and JSX wiring.
