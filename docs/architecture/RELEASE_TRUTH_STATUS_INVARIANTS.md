# Release Truth Status Invariants

Release Truth status is a single lifecycle state per release item. Blockers are
cross-references that explain why incomplete work cannot move, not a second
state flag that can override completion.

## Architecture

Release item status owns lifecycle truth:

- `complete` means the item is done and cannot be blocked.
- `deferred` and `cut` mean the item is outside active release execution and
  cannot be blocked.
- `blocked` means the item is incomplete and blocked by its own status or by an
  active blocker reference.

Blocker records may reference release items only while those items are eligible
to be blocked. When work is completed, deferred, or cut, agents must remove the
item from active blocker `itemIds` or retire the blocker instead of preserving a
contradictory reference.

The systemic enforcement point is Release Truth validation, not viewer styling.
The validator already owns cross-file and cross-field release consistency after
JSON schema validation; it must reject mutually exclusive status relationships
before Release Truth data is treated as valid.

## Documentation Requirements

- Agent instructions must state that `complete`, `deferred`, and `cut` items
  cannot remain in active blocker references.
- Release Truth planning documentation must list blocker/status exclusivity as a
  validation rule.
- The viewer may defensively ignore impossible blocker overlays, but that is
  only a presentation guard. Invalid data must still fail validation.

## Verification

- A release detail whose blocker references a `complete` item fails reference
  validation.
- A release detail whose blocker references a `deferred` or `cut` item fails
  reference validation.
- Release Truth presentation does not display an active blocker state for an
  item whose lifecycle status cannot be blocked.
