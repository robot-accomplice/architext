# Routing Mount-Cost Model — Design (Sub-project A)

- **Date:** 2026-05-29
- **Branch:** `routing-overhaul`
- **Checkpoint (pre-implementation state):** `5a965e1`, 292/292 green
- **Status:** Approved design; pending implementation plan (writing-plans).

## Scope

This spec covers **only sub-project A: the unified mount-cost model.** The wider
routing punch-list was decomposed into independent spec → plan → implement
cycles:

| # | Sub-project | This spec? | Depends on |
|---|---|---|---|
| **A** | Mount-cost model (even-spacing + escape-side) | **Yes** | — |
| B | Centralized viewport-centered popup primitive; migrate **all** centered modals/dialogs onto it; parameterized on duration / acknowledgement / variant | No (own cycle) | — |
| C | Number/decision pills always on top (z-order) | No (own cycle) | — |
| D | Phase 7 progress feedback ("Calculating per-node-pair route costs…") | No (own cycle) | B, A |

Sequence: finish A spec → B → C → D. B/C/D are pure presentation and share no
state with A.

## Problem

Two routing-quality defects, both rooted in the post-selection pipeline at
`viewer/src/routing/routeEdges.js:1012–1015`, which is a sequence of four
single-metric **guarded passes**:

| pass | optimizes | revert guard |
|---|---|---|
| `reorderSharedSurfaceMounts` | mount order matches opposite-node order (↓crossings) | runs only if order differs |
| `routeReciprocalPairsParallel` | return runs parallel to request | revert if collides |
| `reduceCrossingsBySurfaceSwaps` | bubble-sort swap (↓crossings), O(E²)×12 | revert unless crossings drop & no collision |
| `centerSoloReciprocalPairSurfaces` | even-distribute *sole reciprocal-pair* surfaces | revert if bends↑ or collides |

Each guards on **one** metric, so they fight: a swap that removes a crossing can
create a shared segment the next pass cannot see, and even-distribution reverts
whenever it would bend an aligned facing edge. This is why generalizing the
centering pass to all surfaces failed.

**Item 1 — crammed busy surfaces ("5/10").** On a surface carrying several edges
(e.g. `unified-pipeline`), a non-reciprocal pair can end up crammed to ~3px,
because `centerSoloReciprocalPairSurfaces` only handles sole reciprocal pairs.

**Item 2 — hardcoded escape direction.** When a facing edge's direct corridor is
blocked, `escapeSideFor` (`routeIntent.js:138`) picks the perpendicular escape
side by canvas half (`center.y < canvasHeight/2 ? "bottom" : "top"`). Repro:
roboticus `agent-turn-flow`, flow `interactive-turn`, steps 11 (`request-model`)
/ 12 (`model-response`) — the `unified-pipeline`↔`llm-service` reciprocal pair.
`memory-system` sits directly between them in the top row, so both ends escape
to `bottom` and route *under* the blocker through the crowded interior, when the
open top yields a cleaner route.

**Maintainer correction (do not regress):** there is **no center-seeking
preference and no directional bias of any kind** — not toward center, not toward
margin. The only objectives are **legibility and clean routing**. North wins in
the 11/12 case only because it is the objectively cleaner route, not because any
direction is favored.

## Locked decisions

1. **Replace all four passes** with one model. Land it as its own commit(s) on
   top of the checkpoint.
2. **Tiered weighted-sum objective** — one comparable scalar (powers the
   aggregate pass and later incremental deltas). All weights are **named
   constants** in `routeConstants.js` (Rule 3: no magic numbers).
3. **Optimize all movable mounts** (any non-`fixedPort` surface), view-agnostic —
   flows, system-map, dataflow, C4 container/context alike.
4. **No directional bias.** Escape side is a scored candidate, not a rule.
5. **On-demand rebuild seam** for side changes; offsets reuse
   `offsetEndpointRoute`.
6. **Staged local search** (architecture A): per-node-pair → node sweep →
   whole-diagram aggregate accept test.
7. **Optimization last** — v1 recomputes global cost per evaluation (O(E²)
   accepted); incremental crossing-deltas deferred until the outcome is
   validated.

## Architecture

### Module boundary & data model

New module `viewer/src/routing/routeMountModel.js`. `routeEdges.js:1012–1015`
(the four passes) is replaced by a single call:

```js
optimizeMountAssignments(routeById, relationshipById, input);
```

Everything upstream (candidate building, initial per-edge selection, endpoint
stubs) is unchanged. The optimizer takes the already-selected `routeById` as its
**initial assignment** and improves it.

- **Endpoint** = `(relationshipId, endpointIndex)`; movable iff its node has no
  `fixedPorts`.
- **Assignment** = per endpoint, a `{ side, offset }`. The diagram state is fully
  described by every endpoint's assignment plus the route paths they imply.
- **Surface** = `(nodeId, side)`; the set of endpoints on it drives the
  even-spacing term.

### Rebuild seam

To change a *side* (item 2) the optimizer needs a route for the new side, which
is more than an offset shift. The optimizer receives an on-demand callback
`buildRouteForSides(relationship, startSide, endSide)` that runs the existing
candidate builder constrained to those sides and returns the best path. Path
logic stays centralized; the initial selection pipeline is untouched. Offset
moves never rebuild — `offsetEndpointRoute` shifts a mount along its surface — so
rebuilds happen only on side-change moves.

### Cost function

`mountAssignmentCost(routeById, input)` returns the tiered scalar for a
whole-diagram state, reusing existing metric machinery
(`routeIndex.crossingStats`, `sharedSegmentStats`, `bends`, collision checks) and
adding one new term (even-spacing). Tier weights are named constants in a
`MOUNT_COST` block in `routeConstants.js`, with gaps wide enough that a lower
tier can never outweigh a higher one across realistic diagram sizes.

| tier | terms | rationale |
|---|---|---|
| 0 — inviolable | node collision, endpoint-node traversal, off-surface, over-capacity | correctness, never traded |
| 1 | repeated crossings, self-overlap | egregious illegibility |
| 2 | shared segments (count + length) | overlapping lines read as one |
| 3 | crossings | honest crossings (existing 3000-weight) |
| 4 | bends / doglegs | fewer corners = more legible |
| 5 | even-spacing deviation, node clearance, facing-intent mismatch | soft legibility; tie-breakers |

**Even-spacing term (new):** for each surface with `N` mounts over length `L` (the
**full side length** — mounts may run to the corners, matching the existing
`endpointSpreadOffset` formula `((i+1)/(N+1) - 0.5) * L`), ideal slot spacing is
`L/(N+1)`. Cost = summed deviation of each mount from
its ideal slot, **plus a steep sub-penalty (within tier 5) when any adjacent gap
falls below `MIN_LEGIBLE_GAP`** (the ~3px cramming). Because spacing sits below
crossings, the model never creates a crossing to even out spacing, but among
equally-clean layouts it always prefers the evenly-spaced one — the 5/10 fix.

Putting facing-intent at tier 5 (not a hard rule) is the formal expression of
"no directional bias": the model prefers an edge to leave toward its target but
overrides that for any higher-tier gain (a cleaner escape side) — the 11/12 fix.

### Staged search

`optimizeMountAssignments` runs three stages against the single objective,
looping to convergence:

1. **Per-node-pair (stage 1).** For each unordered node-pair sharing ≥1 edge,
   enumerate the small set of side + on-surface ordering assignments for just
   those shared edges and keep the local minimum. Settles reciprocal pairs and
   the escape-side choice.
2. **Node sweep (stage 2).** Walk nodes in stable id order; for each node,
   re-solve the `(side, offset)` of every incident movable endpoint against the
   current neighbor state. Absorbs the old swap-search and reorder passes.
3. **Aggregate (stage 3).** Recompute whole-diagram cost; accept a stage-1/2
   proposal **only if the global scalar strictly drops**. Repeat stages 1–2
   until a full pass accepts nothing, or `MOUNT_MAX_ITERS` is reached.

The accept test lives **only** in stage 3 against one objective — the structural
fix for the fighting passes. Stages 1–2 *propose*; stage 3 *arbitrates*.

**Move application — two kinds:**
- *offset move* → existing `offsetEndpointRoute` (no rebuild).
- *side move* → `buildRouteForSides` rebuild callback.

**Matched movement (straightness rule):** when an offset move shifts an endpoint
belonging to a currently-straight facing edge, the partner endpoint on the
facing node co-shifts by the same delta, keeping the edge straight instead of
bending. This turns the constraint the old code fought ("don't bend the straight
edge") into a move primitive ("move both ends together"), so even-spacing and
straightness stop being in tension.

**Determinism:** every iteration uses stable sort keys (node id, then
relationship id); no `Date`/random. The planner must be reproducible — a
`Map`-iteration-order slip would silently flake the suite.

## Testing & rollout

**Checkpoint first.** Current tip `5a965e1` (292/292 green) is preserved before
A's replacement lands, so any pivot is possible. **If the outcome isn't what we
expect, we stop and discuss how to pivot** (adjust tier weights, change the
search, partial rollback, or full revert) — we do **not** auto-revert. The
checkpoint exists to make that discussion unconstrained, not to predetermine it.

**TDD — write these failing first.** Tests assert legibility *properties*, not
coordinates or compass directions (so the bias cannot creep back in, and the
tests survive future routing changes — Rule 10):

1. **11/12 legibility:** in roboticus `agent-turn-flow` / `interactive-turn`, the
   `unified-pipeline`↔`llm-service` reciprocal pair is crossing-free,
   shared-segment-free, and clears `memory-system`. (North is the surface that
   satisfies this; visually confirm north after.)
2. **Even-spacing (5/10):** on a surface carrying ≥3 mounts, the minimum adjacent
   gap ≥ `MIN_LEGIBLE_GAP` — no ~3px cramming.
3. **Determinism/idempotency:** planning the same view twice yields
   byte-identical routes.

**Protected invariants (must stay green):** no node collisions, no endpoint-node
traversal, no self-overlap, no repeated crossings, perpendicular contact, no
over-capacity. The existing `routing-fitness` suite pins these (tier 0/1).

**Re-baseline policy (Rule 12, fail loud):** tests asserting *specific* crossing
counts or *exact* routes encode incidental behavior of the four old passes; some
will legitimately change. Re-baseline only those, call out each explicitly in the
commit, never weaken an invariant to pass. Aggregate crossings on roboticus must
not regress beyond the current 40 (target ≤40, ideally lower).

**Gate:** `npm run verify` (tests + validate + build + benchmark) plus the
headless roboticus harness must pass before A is considered landed.

## Follow-ups (separate cycles)

- **B** — centralized popup primitive (viewport-centered; migrate all centered
  modals/dialogs; parameterized on duration / acknowledgement / variant).
  `.routing-loading-overlay` / `.routing-planning-error` (`main.tsx:285/295`,
  `styles.css:2229`) currently use `position: absolute` left/top 50% → center on
  the canvas; must be viewport-fixed.
- **C** — number/decision pills always on top (z-order above edges and nodes).
- **D** — Phase 7 progress feedback, rendered through B's popup, reporting A's
  per-stage progress.
