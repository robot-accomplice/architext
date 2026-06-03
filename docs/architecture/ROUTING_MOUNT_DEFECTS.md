# Routing Mount Defects — live-UI review work order

Status: **open work order** for the `routing-overhaul` branch. Produced from a maintainer
live-viewer review on 2026-06-03, after four committed routing improvements
(`d5b3930` NUL fix, `683ba8e` facing-correction pass, `cb18a2a` dogleg weight 3300→6000,
`e1f66b8` shared-corner staircase). Those passed the test suite (308/311) and the mount-audit
metrics — **but the rendered diagrams still have systemic, visible defects the metrics did not
flag.** This document is the catalog and the fix plan; fixing happens in a later session.

## Governing mandate (do not skip)

- **Validate every routing change by reviewing every flow diagram in the live viewer**, not by
  the test suite / crossing counts / mount-audit alone. Metrics diverge from the rendered
  result (proof below).
- **Before tests can be trusted alone, build a defect harness that conclusively surfaces these
  defects AND sanity-check it against screenshots.** The current detector is only partial.
- The agent's inline PNG renders are **not visible to the maintainer**; the browser is the shared
  source of truth. To get feedback, GUIDE the maintainer to the flow (sidebar → FLOWS → name) and
  reference on-screen step numbers.

Live viewer (serves the LIVE `viewer/src`, so it reflects uncommitted changes):

```sh
cd viewer && ARCHITEXT_DATA_DIR=/Users/jmachen/code/roboticus/docs/architext/data npm run dev
# vite, port 4317 or next free; open in browser, hard-refresh after a rebuild-free src edit
```

## Metric-vs-eye gap (why metrics passed)

`doglegCount` only counts segments that **reverse** the from→to direction. It misses the
perpendicular stair-steps (`shallowJog`), same-side "bracket" bows, and uneven/​crowded mounts the
eye reads as defects. Concrete: for `model-inference` in the `agent-turn-flow` view,
`doglegCount = 0` for every edge, yet the maintainer immediately saw jogs on steps 2 and 3. So
"doglegs 21→7, suite green" was measuring something narrower than diagram quality.

## Root-cause synthesis (maintainer)

The recurring complaint set across every reviewed flow reduces to **two primary roots with a
causal chain**:

> **R1 — inconsistent / uneven mount distribution ("for no apparent reason") → crowding →
> R2 — weird face selection** (an edge spills onto the wrong surface *because* the correct face is
> crowded).

- **R1 is standalone and pervasive.** It appears even where face selection is correct
  (`tool-mcp-execution` is "correct except for the weird mount point distribution"). So R1 is the
  universal defect.
- **Fix R1 first** (even mount spread on **all** faces — north/south as consistently as east/west,
  and a lone mount centered). Relieving crowding should also stop most R2 wrong-surface spills.
- **Lane-ordering + missing hops** is a separate, secondary concern (R2-adjacent / rendering).

## Themes (the harness must flag all of these)

| Theme | Description | Notes |
|---|---|---|
| **T1 Distribution** | Mounts not evenly spread along a face; **north/south especially** inconsistent vs east/west; even east/west uneven. | The primary root (R1). |
| **T2 Lone-mount centering** | A single mount on a face is not centered. | Sub-case of T1. `recenterSingletonSideEndpoints` exists — find why it doesn't fire. |
| **T3 Wrong face (crowding-driven)** | Same-column multi-round-trip pairs get pushed to N/S instead of being bracketed onto E/W; far-edge/perpendicular spills. | R2; expected to shrink once T1 is fixed. |
| **T4 Lane order + hops** | Farthest-target line should sit **outermost** to avoid crossings; flat crossings need hop arcs. | Secondary. |

## Per-flow catalog (maintainer eye, mostly `agent-turn-flow` view)

**Model inference and routing**
- Steps 2 & 3 (LLM ↔ Cloud): weird dogleg (caught by `shallowJog`; `doglegCount=0`). [T1/T3]
- North/south faces of LLM don't distribute mounts like its east/west faces. [T1]
- Line 6 (LLM → Observability): routes *inside* lines 4/5 on LLM's right face and crosses them;
  ordering it **outermost** (it targets the farthest node) avoids the crossing — and the crossing
  renders **flat (no hop)**. [T4]

**Memory retrieval, ingest, and maintenance** (improved, but)
- Unified Pipeline ↔ Memory system facing sides not distributed → unnecessary crowding. [T1]
- Line 8 (SQLite → Memory) mounts Memory **south**; should be **west**. [T3]
- Line 7 (Memory → SQLite) mounts SQLite **north**; should be **east**. [T3]
- East faces of Memory & SQLite: uneven mount distribution. [T1]
- Line 8's mount on SQLite's **west** is off-center though it's the only mount there. [T2]

**Skill and plugin lifecycle**
- "Same complaint set" — T1–T4 recur. Harness candidate edges (agent-turn-flow view): 1, 2
  right→right brackets; 5 left→left bracket; 6 excess-bend. (system-map view): 3, 7, 8 excess; 4
  shallowJog. Confirm specifics next session.

**Tool & MCP execution**
- Correct **except** for weird mount distribution. [T1 only — the isolating case.]

**Local CLI/TUI control**
- Weird mount distribution [T1]; no hops on crossings [T4]. (Earlier open Q: SQLite north-vs-west
  mount — part of R2.)

## Suspected code sites (start here next session)

- Distribution / spread / centering — `viewer/src/routing/routeEdges.js`:
  `endpointSpreadOffset` (line ~890), `spreadSharedSideEndpoints` (~955),
  `reorderSharedSurfaceMounts` (~1295), `recenterSingletonSideEndpoints` (~1352),
  `realignFacingEndpoints` (~926), `sideNeedsPostSelectionCentering` (~126).
  NOTE: `sideNeedsPostSelectionCentering` and `endpointSpreadOffset` *do* handle top/bottom, so the
  N/S-vs-E/W asymmetry is **not** there — it is in the facing-alignment / reorder / reciprocal
  pass axis handling. Not yet located.
- Face selection — `viewer/src/routing/routeIntent.js` (`deriveRouteIntent`,
  `expectedRouteSides`, `semanticSurfaceOptions`); reciprocal surface choice in
  `routeMountModel.js` (`reciprocalParallelMoves`).
- Hops — `viewer/src/routing/routeRendering.js` (`pathToSvgWithHops`, `orthogonalCrossings`);
  invoked from `viewer/src/main.tsx`. Crossings render flat — investigate why (hops only fire vs
  *previously-drawn* routes).
- Lane order by destination distance — `reorderSharedSurfaceMounts` orders by opposite-node centre
  but does not control the bracket **depth** ordering that line-6 needs.

## Harness requirement & calibration state

A trustworthy harness must flag T1–T4 and be validated against screenshots before tests are
relied on alone. Current state (partial):
- Candidate detector: `dogleg || shallowJog || bracket(startSide===endSide) || excessBends>2`.
- On `model-inference` it flags steps 2,3 (shallowJog) ✓ and the right→right brackets — but does
  **not** yet distinguish a defect bracket from legitimate obstacle-avoidance, and has no
  **distribution-evenness** or **lone-center** metric yet (T1/T2 uncovered).
- Probes (in `/tmp`, rebuild if cleared): `mount-audit.mjs` (faithful per-flow audit),
  `dg-scan.mjs` (doglegs), `defect-scan.mjs` (per-edge T-flags punch-list), `jog-probe.mjs`,
  `nf-probe.mjs`, `off-probe.mjs`, `render-flow.mjs` (flow → SVG/PNG), `dogleg-sweep.mjs`.

## Suggested fix order (next session)

1. **T1 distribution** (the root): even spread on all faces incl. N/S; lone-mount centering (T2).
   Re-review every flow in the UI; expect R2 wrong-surface spills to shrink.
2. **T3 residual wrong-face** for same-column multi-round-trip pairs (bracket onto E/W).
3. **T4 lane-ordering** (outermost = farthest target) **and hop rendering** on remaining crossings.
4. **Build + calibrate the harness** against screenshots; only then trust tests alone.
