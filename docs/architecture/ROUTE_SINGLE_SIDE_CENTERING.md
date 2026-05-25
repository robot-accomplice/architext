# Route Single-Side Centering

Route endpoint spreading is a per-surface concern. A node may have multiple
relationships overall, but only one relationship attached to a given surface.

## Contract

- A top or bottom side with exactly one selected route uses that side's
  geometric center.
- Endpoint spreading is allowed only among routes sharing the same node side.
- Source and destination top/bottom endpoints follow the same rule.
- This rule applies after the route planner selects sides; unrelated routes on
  other sides must not move a single top or bottom endpoint off center.

This keeps isolated arrows visually anchored to the box they describe while
still allowing fan-in and fan-out stacks where a side actually has contention.
Left and right side centering remains handled by candidate selection and the
existing port tests; this post-selection correction is scoped to the observed
top/bottom regression so it does not collapse established horizontal fan-out
fitness.

## Verification

- A node with one top-side relationship and one left-side relationship keeps
  the top-side relationship centered on the top edge.
