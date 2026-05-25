# Architext 1.4.9 Success Criteria

This document defines success for the Architext 1.4.9 patch release. The
release fixes a Release Path contradiction discovered after 1.4.8: a milestone
could display an incomplete lifecycle status while every linked item was
complete.

## Architecture

Release Path milestone display state is a projection over milestone metadata and
linked item state. Persisted milestone status remains the source data, but the
viewer must not show a weaker state when the linked item rows prove completion.

The projection rule is:

- if a milestone has linked items and all linked items are `complete`, display
  the milestone as `complete`;
- otherwise, if an incomplete milestone has active blocked items, display it as
  `blocked`;
- otherwise, use the stored milestone status;
- milestones without linked items keep their stored status.

This keeps Release Path, collapsed milestone headers, completion counts, and item
rows coherent without mutating target repository data.

## Documentation Requirements

- The collapsible Release Path architecture note records the derived milestone
  display-state rule.
- Release Truth records this patch as a presentation correction.

## Verification

- Presentation-model tests cover completed linked items overriding stale
  milestone status.
- Presentation-model tests cover active blockers for incomplete milestones.
- Empty milestones keep their stored status.
- `npm run release:check` passes before release.

## Out of Scope

- Changing Release Truth schema.
- Rewriting target repository release data.
- Persisting derived milestone status back into JSON.
