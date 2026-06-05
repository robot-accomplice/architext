# Routing overhaul — review handoff

Status: **release-gate bars met; ready for maintainer live-viewer review.** Branch
`routing-overhaul`, 51 commits ahead of `main` (no upstream set yet). The routing module under
`viewer/src/routing/` is effectively new on this branch (24 files, ~6.5k lines: a routing-engine
extraction plus the mount-model, distribution, diagnostics, and hop work).

## Validation (run 2026-06-05)

- **Suite: 323/326 pass** (`npm test`). The 3 reds are characterized below — none is a crash or a
  corpus-flow regression.
- **Benchmarks: 12/12** (`npm run test:benchmark`).
- **Architext data: validates clean** (`architext validate`).

## Release bars — both met

The maintainer's gate (everything else is bonus):

1. **0 avoidable doglegs — MET.** 0 reachable shallow jogs; the 4 remaining doglegs are all
   collision-forced (the direct mount approach passes through a node). Reachable-views-only audit:
   `/tmp/reachable-jogs.mjs`.
2. **All crossings have hops — MET (session 9, `a84928e`).** Corpus-wide hop coverage is **236/236**
   (every one of 118 geometric crossings hops from both sides; zero deficit). The last gap was the
   vertex-crossing case (a crossing landing on a sibling's collinear waypoint) — fixed by merging
   collinear runs before hop detection. Sweep: `/tmp/hop-coverage.mjs`.
3. Ordering / crossing-reduction — **bonus**, not gating.

## How to review in the live viewer

```sh
cd viewer && ARCHITEXT_DATA_DIR=/Users/jmachen/code/roboticus/docs/architext/data npm run dev
# → http://127.0.0.1:4317 (Vite picks the next free port if taken)
```

Drive the left-nav **Flows** list (per-flow Flow Views). Screenshots time out on dense views —
extract rendered geometry from the DOM instead: `g.flow-edge path.flow-line` `d`-attribute; a `Q`
command is a rendered hop. Flow-card `.click()` is async (React) — read the result in a separate
step.

**Witness to confirm the session-9 fix:** open **Model inference and routing**. `record-route`
(L6, →observability) descends the gutter at x≈1102 and now hops twice — over `route-local` (L4) at
y≈492 and `local-provider-result` (L5) at y≈474. Both L4 and L5 hop once over L6. Previously these
three rendered flat.

## Known residual BONUS defects (UI-confirmed, do not gate the bar)

- ✅ **skill-plugin steps 3 & 7 mount overlap — FIXED (`e26001a`).** Width-aware slot spacing
  (`spreadUnitSlots`) reserves each unit's width so adjacent reciprocal pairs no longer collide;
  left-face mounts now spread to 7.5px gaps (was 1.5px), live-DOM verified.
- ✅ **Crossing-minimizing unit order — LANDED (`e26001a`).** The same pass now orders units by where
  they LAND on the far node, so reciprocal bundles between the same node pair stay parallel instead
  of swapping. Corpus: total crossings **118 → 64**, pair-internal **24 → 10**, lane-order **11 → 6**,
  with identical total bends and zero shared segments (pure ordering, no added contortion). This is
  the practical core of the maintainer's "pair-aware ordering" ask, integrated with distribution.
- **Remaining reorder opportunities** — 64 crossings remain; some are further reorderable (e.g.
  same-source fan-outs), some are inherent cross-lane layout crossings, and pair-internal (10) is the
  straightening pass's job. The gutter-lane "farthest target → outermost" rule is not yet applied.
- **T3 surface-selection jogs** — `memory-lifecycle` L7/L8 and `skill-plugin` L6 pick a crowded/remote
  face instead of the near one. Face *selection* in `routeIntent` / `optimizeMountAssignments`.
- **Two hop deficits** — `system-map/interactive-turn` and `system-map/mail-operations` each have a
  few crossings that render flat. These appear only in the system-map projection (likely phantom for
  flows that have an authored view); a hop-rendering edge case, not a crossing-count problem.
- **Min-segment-length stubs** — 89 node-hugging segments under 12px. A legibility nicety.

## The 3 failing tests — honest read

1. **`route-ports.test.mjs:152` "single flow routes stay centered on their selected system map
   surface."** Of its two assertions, `write-starter-data` passes; `write-metadata` fails because it
   now mounts the **left face** of `target-repository` (`{810,94}`) instead of the old
   **top-center** (`{878,76}`). This is a deliberate face-selection change from the overhaul, not a
   crash — the test encodes a pixel-exact pre-overhaul expectation. **Maintainer call:** eyeball the
   "Fresh data-only install" System Map flow and either bless the new face (update the assertion) or
   treat it as a face-selection bug.
2. **`routing-diagnostics.test.mjs:94` "dense fan-in diagnostics explain surface-capacity escape
   endpoints."** Synthetic stress view (`Complex Fan-In`). Fails at the assertion that a
   blocker-coplanar source escapes to a *perpendicular* (top/bottom) gutter; the router still exits
   left/right here. Aspirational obstacle-aware-escape property, unmet on this synthetic case.
3. **`routing-diagnostics.test.mjs:152` "semantic return gutters leave badge-sized clearance between
   long parallel lanes."** Synthetic; asserts `closeParallelRuns == 0` (badge-sized clearance)
   between long parallel lanes. Unmet spacing ideal on the synthetic case.

Tests 2–3 live in `routing-diagnostics.test.mjs`, which is **new on this branch** — they are
branch-authored quality targets the router does not yet hit on synthetic dense cases, not
regressions against real corpus flows. Recommend deciding per test whether to (a) relax to the
achieved behaviour, (b) keep as a known-failing aspiration, or (c) schedule as bonus work.

### Red #1 root cause (session-9 investigation — needs a maintainer design call)

`write-metadata` (architext-cli → target-repository, same row) and `install-valid` (schema-validator
→ target-repository) both resolve to `target-repository.left` via the forward-lane intent rule, so
the left face carries two mounts (94 / 112). Two distinct problems compound:

1. **`doglegCount` blind spot (a real bug, fixable in isolation).** `doglegCount`
   (`routeMountModel.js:85`) derives `yDir = sign(to.y − from.y)`. For a same-row pair `yDir = 0`, so
   *all* vertical excursion is uncharged. `write-metadata` wanders up to y=46 (above the row) and back
   — an up-and-over the optimizer cannot see. A clean left-face path exists at excess≈9 vs the
   current ≈105; fixing the blind spot (penalize off-axis excursion when the partner direction on
   that axis is 0) would let the optimizer straighten it. This change is corpus-wide and should be
   regression-checked on roboticus before landing.

2. **Face-spread policy (a design call).** Straightening still leaves `write-metadata` on the *left*
   face; the test wants the *top* face so each of the two edges is centered on its own surface. The
   weighted cost model has no "spread a node's edges across faces for centering" term, and top
   actually costs more length, so it prefers the stacked left face. **Decision needed:** add a
   face-spread/centering preference (this is the T3 surface-selection work), or bless a clean stacked
   left face and update the test's expectation to the straightened left-face mounts. Recommend the
   maintainer eyeball "Fresh data-only install" on System Map and choose.

## Parked

- **Phase7-A WIP** is in `stash@{0}` (T6 integration + per-node-pair bridge stage + soft-cap + cost
  telemetry; "regresses vs four-pass" — kept for the cost-calculus rework). Not part of this review.

## Repro tools (all use flows.json steps + planDiagram, the production path)

- `/tmp/hop-coverage.mjs` — corpus crossings vs rendered hops (236/236).
- `/tmp/backtrack-scan.mjs` — proves the collinear merge is geometry-identity (303 routes, 0
  backtracks).
- `/tmp/mi-hops.mjs`, `/tmp/mi-dump.mjs` — per-route geometry/hops for model-inference.
- `node viewer/tools/audit-route-crossings.mjs --data-dir <dir>` — committed pair-internal /
  lane-order crossing sweep.
