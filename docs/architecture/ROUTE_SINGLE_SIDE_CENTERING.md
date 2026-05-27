# Route Single-Side Centering

Route endpoint spreading is a per-surface concern. A node may have multiple
relationships overall, but only one relationship attached to a given surface.

## Contract

- A top or bottom side with exactly one selected route uses that side's
  geometric center.
- Endpoint spreading is allowed only among routes sharing the same node side.
- A side with multiple route endpoints must spread those endpoints on that
  surface with enough visual clearance that arrowheads do not read as a single
  collision. Two routes must not claim the same anchor point unless the node
  uses fixed ports.
- Shared-side endpoints are distributed by cardinality. For `n` connections on
  one surface, endpoint `i` lands at `(i + 1) / (n + 1)` along that surface; two
  connections therefore use the one-third and two-thirds points.
- For east and west surfaces, mount points are distributed along the
  north-south axis. For north and south surfaces, mount points are distributed
  along the east-west axis.
- When a distributed endpoint connects to an opposite movable surface, move the
  opposite endpoint to the same mount axis position only when the resulting
  route remains strictly orthogonal.
- If both surfaces have their own endpoint groups, each surface keeps its local
  distribution. Cross-surface alignment is best-effort and must not overwrite a
  crowded opposite surface's own mount-point distribution.
- Cross-surface alignment must also avoid creating a new shared segment with
  another visible route; when alignment would reuse an occupied corridor, keep
  the distributed endpoint and let the route dogleg.
- Endpoint adjustment must orient the first bend before the arrowhead in line
  with the arrowhead mount position; do not add a corrective double right angle
  at the final stub.
- When the existing upstream segment is already north-south into an east/west
  surface, move that upstream bend to the mount y-position instead of routing to
  the old bend and then backtracking north/south.
- Visual line hops are assigned only to north-south route segments. East-west
  segments must remain straight so primary left-to-right relationships stay
  readable.
- Source and destination top/bottom endpoints follow the same rule.
- This rule applies after the route planner selects sides; unrelated routes on
  other sides must not move a single top or bottom endpoint off center.
- Post-selection endpoint spreading must preserve the selected line style. In
  orthogonal mode, spreading an endpoint may add or move an elbow, but it must
  not create a diagonal segment.
- Routing cost weights, canvas boundary insets, and generic deterministic
  helpers such as rectangle centering and keyed de-duplication must live in one
  shared routing module. Candidate builders, route strategies, corridors, and
  route post-processing import those values instead of copying numeric literals,
  so later tuning is a deliberate policy change rather than an accidental
  refactor side effect.

This keeps isolated arrows visually anchored to the box they describe while
still allowing fan-in and fan-out stacks where a side actually has contention.
Left and right side centering remains handled by candidate selection and the
existing port tests; this post-selection correction is scoped to the observed
top/bottom regression so it does not collapse established horizontal fan-out
fitness.

## Verification

- A node with one top-side relationship and one left-side relationship keeps
  the top-side relationship centered on the top edge.
- A node with multiple relationships entering the same side receives visually
  distinct anchors on that side.
- Opposing movable surfaces align to the same mount point for the route.
- Orthogonal routes remain orthogonal after endpoint centering or spreading.
