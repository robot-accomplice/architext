# Architext 1.4.10 Success Criteria

This document defines success for the Architext 1.4.10 patch release. The
release packages the post-1.4.9 viewer and lifecycle fixes that were merged
after the 1.4.9 publish.

## Architecture

Serve discovery reports live serve processes, not only detached background
processes. Foreground and background serve processes both write runtime state,
and foreground records are intentionally non-restartable because the owning
terminal controls their lifecycle.

Diagram header overlays belong above the contained canvas viewport. The canvas
may isolate its own paint and scrolling behavior, but it must not hide header
controls that intentionally open downward.

Flow projections and selected flows have a compatibility invariant: the
selected flow view must contain every endpoint node used by the selected flow.
When a broad overview and a narrower authored projection can both render a
selected flow, the UI prefers the narrower projection to avoid misleading
floating paths. Sequence diagrams are selected-flow artifacts and title
themselves from the selected flow.

## Documentation Requirements

- Serve lifecycle documentation describes live serve instances rather than
  background-only discovery.
- Architecture documentation records legend overlay ownership.
- Architecture documentation records Flow View and Flow compatibility rules.
- Release Truth records the patch scope and verification.

## Verification

- `architext --list --json` discovers a live foreground serve process.
- CSS tests assert the legend panel stacks above the canvas viewport.
- View-selection tests assert flow/view compatibility, filtering, and repair.
- Browser verification against the Roboticus data set confirms selecting a flow
  from the broad System Map moves to a compatible authored Flow View.
- `npm run release:check` passes before release.

## Out of Scope

- Changing the Architext data schema.
- Persisting explicit flow-to-view links in target JSON.
- Rewriting target repository view or flow data.
