# Routing overhaul — review handoff

Status: **release-gate bars met; PR #81 open; pushing for release.** Branch
`routing-overhaul` (pushed, tracking `origin/routing-overhaul`). The routing module under
`viewer/src/routing/` is effectively new on this branch (24 files, ~6.5k lines: a routing-engine
extraction plus the mount-model, distribution, diagnostics, and hop work).

---

## ⮕ NEXT SESSION — START HERE (handover 2026-06-05, session 9)

**State:** working tree clean, all pushed, PR https://github.com/robot-accomplice/architext/pull/81
OPEN. Phase7-A WIP parked in `stash@{0}` (do not lose). Both release bars MET (0 avoidable doglegs;
all crossings hop). Crossing-reduction (bonus) made big strides this session.

**This session's work (all committed & pushed):**
1. `a84928e` vertex-crossing hops — merge collinear runs before hop detection (bar 2 closed).
2. `e26001a` distribution: width-aware spread (`spreadUnitSlots`) + crossing-minimizing unit order
   (order by far-endpoint landing, not display index).
3. `2b6b0b5` gutter-lane order (`orderGutterLanesByTarget`) — lone farthest-target edge takes the
   outermost lane (fixes model-inference edge 6 / `record-route`). Guarded final pass.

**Crossing metrics (roboticus), baseline → now:** rendered crossings **48 → 23**, all-views
**118 → 54**, lane-order violations **11 → 3**, pair-internal **24 → 10**, with bends ~unchanged
(571 → 569) and **0 shared segments** throughout — pure reordering, no added contortion.

**CI is RED** (https://github.com/robot-accomplice/architext PR #81 `verify` job). 4 failures:
- `routing-mount-cost.test.mjs` — **hardcoded local path** `const ROBO = "/Users/jmachen/code/
  roboticus/…"` (line 6). Passes locally (roboticus is on this machine), fails CI. **Clear fix:**
  convert to an in-repo fixture or skip-when-absent. Not a design call. Pre-existing (commit `7387f13`).
- `single flow routes stay centered…` — asserts exact coords on the **system-map** projection of
  `fresh-install`, but that flow renders in `dataflow`/`workflow` (authored views), so system-map is a
  **phantom** users never see. Rescope the test to a rendered view, or relax. Root cause also has a
  real `doglegCount` yDir=0 blind spot (see below) — fixable but optional.
- `dense fan-in diagnostics…`, `semantic return gutters…` — branch-authored **synthetic aspirations**
  the router doesn't meet; not corpus regressions. Relax to achieved behavior or keep known-failing.

**Open work, prioritized for release:**
1. **Green CI (path A, the maintainer chose this):** fix the hardcoded-path test (clear), then decide
   the 3 reds (rescope/relax with justification — do NOT mask a real defect).
2. **`memory-lifecycle` "still ugly" (maintainer-flagged in visual pass):** 3 crossings in the
   Memory↔SQLite bundle. Partly **layout-forced** (3 nodes — product-knowledge, skill-plugin, mcp —
   stacked between Memory and SQLite force the detour). Partly **face-selection / T3**: the `curate`
   pair (curate-repair/curation-result, steps 7/8) spills onto remote faces (sqlite.top, memory.bottom)
   instead of bracketing the east faces like the query pair. T3 = face SELECTION in
   `routeIntent`/`optimizeMountAssignments`; no T3 detector exists yet.
3. **Remaining reorder levers (exhaust before alternatives — maintainer rule):** (a) 2-layer
   consistent ordering for same-source-face bundles whose two faces still disagree after the
   far-landing sort (sameSourceFace rendered = 3); (b) more `orderGutterLanesByTarget` cases (it
   currently handles a single lone crosser per face — could generalize to multi-edge combs); (c) the
   8 rendered pair-internal crossings are `straightenSelfCrossingPairs`'s non-facing misses.

**Maintainer rules (standing, do not violate):**
- ALWAYS exhaust reordering to reduce crossings BEFORE alternative solutions (guards, layout), and
  treat distribution(spread) + ordering as ONE crossing solution.
- Validate EVERY routing change in the LIVE UI (the browser is the shared truth); the agent's
  screenshots time out, so extract `g.flow-edge path.flow-line` `d`-attributes from the DOM instead.
- Node geometry is NOT a routing lever (never resize nodes to fix edges).

**How to run / verify (critical gotchas):**
- Live viewer: `cd viewer && ARCHITEXT_DATA_DIR=/Users/jmachen/code/roboticus/docs/architext/data npm
  run dev`. **A bare `npm run dev` serves NO data → the viewer drops into "recovery mode"** (the
  ARCHITEXT_DATA_DIR env var is required; `vite.config.ts:61` returns early without it). This is NOT
  data corruption.
- Drive flows via the left-nav flow cards; flow-card `.click()` is async (React) — read selection in a
  SEPARATE `browser_evaluate` call.
- `viewer/src/routing/routeEdges.js` contains **NUL bytes** — plain `grep` silently skips it; use
  `grep -a` / `grep -c`.
- Run `git` pathspecs from the **repo root** (a `git stash push -- viewer/…` from inside `viewer/`
  mis-resolves the path and can pop the wrong stash — happened this session, recovered).
- The distribution guard `keepMountMovesUnlessWorse` checks bends/collision/shared-segment but NOT
  crossings; the new `orderGutterLanesByTarget` has its own crossing-aware guard.
- Repro/measure tools (all use flows.json steps + planDiagram, the production path):
  `/tmp/reach-cross.mjs` (rendered-view crossings, classified), `/tmp/hop-coverage.mjs` (all-views
  crossings + hop deficits), `/tmp/health.mjs` (bends + shared segments), `/tmp/classify.mjs`
  (crossing buckets), `/tmp/mi-dump.mjs` (model-inference geometry),
  `node viewer/tools/audit-route-crossings.mjs --data-dir <dir>` (pair-internal + lane-order, committed).
- Tests: `npm test` (suite), `npm run test:benchmark` (benches 12/12), `architext validate`.

---

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
- **Remaining crossings (rendered views only — what users actually see).** Of the all-views 64, only
  **27 are in reachable/rendered views** (the rest are phantom system-map projections of flows that
  render in an authored view). The face-distribution reorder cut rendered crossings **48 → 27**.
  Remaining rendered 27 break down as: **8 pair-internal** (the `straightenSelfCrossingPairs` pass's
  job — these are its non-facing / multi-bend misses), **4 cross-lane** (different node pairs in
  different lanes — inherent, not reorderable), and ~15 face-shared. The bounded face-distribution
  reorder is *complete*. (Post-comb-pass figures: rendered crossings now **23**, all-views **54**.)
  Remaining reorder levers:
  - ✅ **Gutter-lane order ("farthest target → outermost") — LANDED (`2b6b0b5`).**
    `orderGutterLanesByTarget` reroutes a lone farthest-target crosser onto the outermost lane; fixed
    `model-inference` `record-route` (edge 6), crossing-free, live-DOM verified. A guarded final pass
    with its own crossing-aware guard — NOT a candidate-selection rework (lower risk than feared).
    Could still generalize to multi-edge combs.
  - **2-layer consistent ordering** for same-source-face bundles whose two faces still disagree after
    the far-landing sort (needs an iterative barycenter-style pass). Rendered sameSourceFace = 3.
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
