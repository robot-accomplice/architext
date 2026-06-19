# Diagram Fit Contract

The diagram `Fit` action is a rendered-surface operation, not a view-mode
heuristic. It must use the currently displayed diagram's measured canvas and the
currently visible scroll container.

Fit means:

- choose the largest zoom that keeps the full rendered canvas inside the current
  scroll container without requiring horizontal or vertical scrolling;
- calculate from the active scroll shell's `clientWidth` and `clientHeight`, not
  from window size or mode presets;
- calculate from the active canvas's unscaled diagram dimensions, not from the
  already-scaled CSS box;
- allow zoom below the normal readability floor when that is required to avoid
  scrolling;
- clamp only to the viewer's supported zoom bounds so very small diagrams do not
  grow without limit.

The rendered diagrams must expose their unscaled canvas dimensions to the shared
fit logic. Flow, C4, sequence, loading, and error canvases all use the same
surface contract so individual renderers do not invent separate fit behavior.

Diagram overlays use named stacking layers rather than an unbounded
`always-on-top` escape hatch. The contained canvas owns diagram paint and scroll
behavior, but app-level transient notices, header overlays, side toggles, and
modal recovery dialogs must render above it through explicit layer tokens.
Canvas-local content must not require arbitrary z-index escalation to be
readable.
