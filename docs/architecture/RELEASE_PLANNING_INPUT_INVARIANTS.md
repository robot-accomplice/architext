# Release Planning Input Invariants

Release Planning domain helpers are deterministic transformations. Adapters may
provide clocks and filesystem effects, but the domain layer must not invent
time or silently ignore malformed selection input.

## Contract

- `buildReleasePlan` requires an explicit `now` timestamp from the adapter.
- Every `selectedRoadmapItemId` must match a roadmap item. Unknown IDs fail
  loudly before a plan is built.
- Release items copy mutable arrays from roadmap or ad hoc input. Returned
  `dependsOn` and `evidence` arrays must not share references with source
  objects.

This keeps the domain layer replayable and makes stale UI selections visible
instead of silently dropping release scope.

## Verification

- Calling `buildReleasePlan` without `now` throws.
- Selecting an unknown roadmap item ID throws.
- Mutating generated release item arrays does not mutate the source roadmap
  item arrays.
