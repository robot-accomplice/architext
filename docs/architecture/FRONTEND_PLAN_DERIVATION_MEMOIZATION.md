# Frontend Plan Derivation Memoization

Diagram route planning consumes derived view state: visible node sets,
relationships, layout constants, and selected-edge ordering. These derivations
are pure, but they sit in render paths that also respond to zoom, pan, focus,
and selection.

## Contract

- Structural and flow relationships are recomputed only when their source model
  inputs change.
- Route-planning inputs keep stable object identity across unrelated render
  changes.
- Fallback canvas geometry follows the same memoized planning input.
- Selected relationships may reorder rendered edges without invalidating the
  route-planning worker.

Selection affects draw order and styling. It does not change route geometry.

## Verification

- Existing route-planning and view-mode tests continue to pass.
- The app build type-checks the hook dependency boundaries.
