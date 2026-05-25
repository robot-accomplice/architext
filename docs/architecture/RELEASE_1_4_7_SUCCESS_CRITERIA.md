# Architext 1.4.7 Success Criteria

This document defines success for the Architext 1.4.7 patch release before the
release candidate is cut. The release is intentionally narrow: Release Truth
must not display a completed item as blocked, even when stale blocker references
remain in the data.

## Architecture

Release item lifecycle status is authoritative. A blocker is supporting context
for active, incomplete work; it is not an independent state flag that can
override completion.

The systemic fix has two layers:

- Presentation derives effective blockers through one shared helper before
  rendering Release Path rows, Kanban cards, milestones, or detail panes.
- Validation rejects active blocker references to `complete`, `deferred`, or
  `cut` release items so contradictory data cannot be accepted as valid Release
  Truth.

This release must not add a second task model or special-case individual UI
surfaces. Release Path, Kanban, and detail rendering must agree because they use
the same presentation rule.

## Documentation Requirements

- Architecture notes describe the Release Truth status invariant.
- Agent-facing Release Truth instructions state that completed, deferred, and
  cut items must be removed from active blocker `itemIds`.
- Release Truth planning documentation lists blocker/status exclusivity as a
  validation rule.

## Verification

- Presentation tests prove completed, deferred, and cut items do not show active
  blocker overlays.
- Kanban tests prove stale blocker references cannot move completed items out of
  the Complete column or mark active items blocked through retired blockers.
- Reference validation tests prove blockers cannot reference complete, deferred,
  or cut items.
- `npm run validate` passes.
- `npm test` passes.
- `npm run build` passes.
- Browser smoke on `#releasetruth` loads without console errors and shows
  completed items as complete, not blocked.
- `npm run release:check` passes before release.

## Out of Scope

- Publishing, tagging, or creating a GitHub release.
- Changing Release Truth schema enums.
- Changing release planning write behavior beyond validation of impossible
  blocker/status combinations.
