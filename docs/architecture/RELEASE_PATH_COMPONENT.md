# Release Path Component

Release Path is the largest remaining Release Truth projection inside
`main.tsx`. It also owns the interaction surface that will support collapsible
release schedule elements.

## Architecture

`presentation/ReleasePathView.tsx` owns the Release Path projection:

- milestone rows;
- item rows;
- path numbering;
- item scope/workstream labels;
- active blocker display;
- selection callbacks for milestones and items.

Shared blocker grouping moves into `presentation/releaseTruth.js` because Release
Path, Release Kanban, and Release Truth details all need the same item-to-blocker
projection.

`main.tsx` keeps top-level selection state and passes callbacks into the
extracted component. Collapse state is intentionally left for the next UX slice
so this PR stays a behavior-preserving extraction.

## Verification

- The extracted Release Path keeps the same props and selection callbacks.
- Shared blocker grouping has direct presentation-model coverage.
- The frontend build validates TypeScript imports and JSX wiring.
