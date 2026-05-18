# Architext Routing Correctness Plan

Architext routing is a correctness subsystem. It should be developed and tested
as geometry, not tuned only by looking at screenshots.

## Goals

- Keep edges out of node bodies.
- Keep labels out of node bodies and away from other labels when practical.
- Make fan-out and fan-in deterministic for repeated source/target groups.
- Keep route output stable for identical model data.
- Keep selected routes visually traceable.
- Allow users to choose a single route rendering style per view: orthogonal or
  spline. Spline mode means curved edges derived from accepted route geometry;
  it must not be only orthogonal routing with rounded corners.
- Prefer automatic routing. Data-level hints should only influence scoring when
  the automatic result is not good enough.

## Non-Goals

- No manual per-edge coordinate authoring as the default workflow.
- No browser-only routing behavior that cannot be exercised from tests.
- No layout rewrites until current routing behavior is isolated behind a pure
  API.

## Target API

The viewer should call a pure diagram planning function before drawing anything:

```ts
planDiagram(input: DiagramPlanningInput): PlannedDiagram
```

`planDiagram` should see the whole rendered diagram:

- view lanes and lane bounds
- node rectangles
- relationship set
- expected label text and approximate label boxes
- current route style
- canvas bounds
- reserved UI bands and gutters
- routing/debug options

The edge router remains a subordinate pure function:

```ts
routeEdges(input: RoutingInput): Map<string, RoutedEdge>
```

`RoutingInput` should include:

- relationships to route
- node rectangles
- visible node ids
- lane and row indexes
- canvas bounds
- route options such as node padding, label padding, and debug mode

`RoutedEdge` should include:

- edge id
- SVG path string
- label point
- route samples
- total cost derived from named route-quality costs
- route-quality cost components for length, boundary pressure, node clearance,
  edge proximity, crossings, repeated crossings, bends, doglegs, perimeter
  fallback, fan-out direction, label movement, and label conflicts
- warnings when no clean route exists
- optional debug metadata such as rejected candidates and collision scores

`PlannedDiagram` should include:

- planned node rectangles
- planned lane bands
- routed edges
- label positions and label boxes
- warnings for node density, too-close nodes, least-bad routes, and label
  conflicts
- debug geometry for corridors, ports, and rejected candidates

## Invariants

The routing test suite should encode these invariants:

- Every routed edge has finite numeric coordinates.
- A rendered view must not mix orthogonal and spline route styles. Orthogonal
  mode renders axis-aligned connector segments with hop-overs; spline mode
  renders spline paths consistently.
- Every route has a stable path for stable input.
- Source and target anchors are outside or on the boundary of their nodes.
- The first and final route segments meet source and target node boundaries at a
  perpendicular angle.
- Routes avoid unnecessary bends, doglegs, and corridor excursions when a
  straighter clean route exists.
- Candidate generation must stay bounded. Flexible ports are useful only if they
  do not make dense real-world views too slow to validate.
- Perpendicular line crossings should use hop-over rendering when the crossing
  is accepted rather than avoidable.
- Crossing the same route more than once is almost always a planner failure and
  should be heavily penalized before hop-over rendering is considered.
- Multiple routes using the same node side should not emerge from the exact same
  surface point unless color, z-order, and selection highlighting make the stack
  unambiguous.
- Perpendicular contact does not require anchoring to the center of a node side.
  The planner should choose among valid points along a side when that avoids an
  unnecessary bend.
- Short middle jogs between two parallel route segments are route-quality
  failures. The planner should choose a better side or port instead of drawing a
  shallow Z break.
- Labels and step badges must not obscure the beginning or end of short
  connectors. For short straight connectors, place the badge beside the line
  rather than centered on it.
- Flow step badges are part of the route, not free-floating labels. They may
  move along sampled route geometry to avoid collisions, but they must stay
  attached to the line. Structural relationship text may use freer label
  placement when needed.
- Port spacing must not introduce a dogleg into a clean direct route. Prefer a
  centered direct connector over an offset connector when there is no overlap to
  resolve.
- Route samples avoid non-endpoint node rectangles with configured padding when
  a clean route is available.
- When no clean route exists, the router reports a warning instead of hiding the
  failure behind a convoluted path. In practice, this often means nodes are too
  close together or the view is too dense for the current layout.
- Multi-edge fan-out creates distinct routes or label positions.
- Labels avoid non-endpoint node rectangles when an alternative exists.
- Route order is deterministic and independent of JavaScript map iteration
  accidents.
- Viewer route planning that takes longer than one second must show visible
  progress feedback. Long planning must not leave the viewer looking frozen.

## Viewer Responsiveness

Route planning is pure geometry, but the viewer must treat it as potentially
expensive work. A mature global CLI has to handle large target repositories
without making the browser appear broken.

The viewer should plan routed diagrams through a single asynchronous boundary
instead of calling the planner directly from React render. The first practical
boundary is a package-owned Web Worker:

- React builds the complete `planDiagram` input for the active view.
- A worker runs `planDiagram` and returns structured-cloneable geometry.
- The main thread reconstructs view helpers such as `positionFor` from returned
  node rectangles.
- A route-planning overlay appears only after a plan has been pending for more
  than 1000 ms.
- Fast plans should not flash a loading state.
- Worker failures should render a visible route-planning error instead of
  silently leaving stale geometry on screen.

This is a viewer-responsiveness rule, not a substitute for routing performance
work. Self-contained synthetic benchmarks should still ratchet planner runtime
downward, but any runtime above one second must be made explicit to users.

## Fixture Catalog

Initial fixtures:

- `simple-adjacent`: two nodes in neighboring lanes.
- `same-lane`: source and target in one lane.
- `multi-edge-fan-out`: one source routes to multiple targets.
- `multi-edge-fan-in`: multiple sources route to one target.
- `bidirectional`: opposite relationships between the same pair.
- `dense-lanes`: blockers between source and target lanes.
- `long-label`: label placement under wider text.
- `c4-component`: structural dependency view with container/component cards.
- `data-risks`: routes in the risk overlay view.

## Fitness Tests

Manual experiments outside the repository may inform future fixture design, but
they are too broad and too variable to be the primary routing standard. Routing
correctness should be protected by named synthetic fixtures that are dense
enough to expose planner failures and small enough to run on every local test
pass.

Default local and CI tests should run the committed fixture suite so normal
routing iteration stays fast and deterministic.

Fitness tests should operate on planned geometry, not screenshots. Each fixture
should assert the same invariants that define acceptable output:

- route coordinates are finite and deterministic
- routes do not enter non-endpoint node rectangles
- source and target contact is perpendicular
- clean direct routes stay straight
- fan-out and fan-in use distinguishable attachment points or labels
- accepted perpendicular crossings render hop-overs
- a route does not cross the same route more than once
- bend counts stay under fixture-specific limits
- labels stay outside node bodies when the fixture has enough space
- fixture-level metric budgets stay within agreed bounds for bends, crossings,
  repeated crossings, dogleg cost, label movement, label conflicts, and warning
  counts
- perimeter fallback routes are warnings, not invisible successes; fixture
  budgets should ratchet allowed fallback counts downward as interior routing
  improves
- monotonic backtracking is now a named route-quality cost. Current complex
  fixtures have zero backtracking, which means the remaining fallback problem is
  corridor availability rather than path direction alone.
- interior corridor candidates now reduce `complex-fan-out` perimeter fallback
  routes from three to two. Perimeter fallback now considers the full port
  candidate set, which removed the remaining `complex-fan-out` endpoint stack
  without increasing fallback count.
- Route scoring now evaluates an estimated label box, not only the route label
  anchor point. This keeps label readability in the same candidate-selection
  pipeline as route geometry instead of relying solely on post-placement repair.
- Interior candidate generation must consider whole-diagram free-space gutters,
  not just the midpoint gap between the source and destination rectangles. Dense
  fan-out and fan-in diagrams often have a clean lane gutter between blocker and
  endpoint columns; treating that as a first-class interior corridor avoids
  perimeter fallback without adding per-fixture route hints.
- Endpoint stack detection is symmetric. Fan-out must separate source anchors,
  and fan-in must separate destination anchors before bend count is allowed to
  break ties.
- Corridor candidate generation is bounded to the source-target span and route
  point sequences are deduplicated before scoring. This preserves whole-diagram
  gutter awareness without forcing every edge to evaluate every corridor in the
  diagram.
- Cheap direct and gutter candidates are scored before Dijkstra grid candidates
  or perimeter fallbacks are generated. Grid/perimeter routing remains available
  for hard cases, but clean cheap candidates short-circuit the expensive path.
- Edge-proximity scoring must not use pairwise sample scans in the main routing
  loop. Until it is backed by a spatial index, correctness checks rely on
  collisions, crossings, repeated crossings, endpoint stacks, doglegs, and
  fallback warnings.
- Dense benchmark after cheap-candidate short-circuiting, bounded corridors,
  grid side-pair pruning, and disabled pairwise edge-proximity scans: 69 seconds
  on May 14, 2026. Previous successful benchmark was 409 seconds; intermediate
  attempts that kept pairwise edge-proximity scans exceeded ten minutes.
- Next optimization target: replace repeated previous-route scans with a route
  spatial index. Candidate scoring should query only nearby prior route samples
  or segments instead of walking every previous route for every candidate.
- Route crossing and endpoint-stack checks now use an incremental route index.
  dense benchmark after this change: 27.8 seconds on May 14, 2026, down from 69
  seconds after the first optimization pass and 409 seconds before routing
  optimization.
- Next optimization target: index node rectangles for route quality, label
  clearance, and collision checks. Candidate scoring should query nearby
  blockers by sample bounds instead of scanning every non-endpoint node for
  every sample.
- Blocker rectangle indexing was tested after the route index and did not improve
  the dense benchmark enough to keep as the next retained optimization.
  The next retained target is the grid router's Dijkstra implementation: it
  should use a priority queue instead of repeatedly scanning every graph point.
- Priority-queue Dijkstra did not materially improve the dense benchmark;
  it remains useful as bounded algorithmic cleanup for hard grid-route cases.
  The dominant repeated work was route planning the same geometry for orthogonal
  and spline render styles. Raw route geometry is now cached independently of
  style so a style change only re-renders the path shape. Dense benchmark after
  raw-route caching: 15.4 seconds on May 14, 2026.
- Subsequent local dense benchmark runs after adding worker-backed viewer
  planning still passed but measured 20.5-29.5 seconds. The worker change
  improves viewer responsiveness rather than pure planner speed; broad manual
  experiments remain too slow and variable to run by default.
- CPU profiling shows the retained hot path is route-clearance scoring:
  `distanceToRect`, `routeQualityFromSamples`, grid-route segment checks, and
  test collision verification dominate runtime. The next retained optimization
  should preserve route semantics while reducing repeated blocker lookup and
  avoiding square-root distance work until a point is within a clearance
  threshold.
- Retained clearance optimizations now cache blocker rectangles per endpoint
  pair, prefilter blockers by candidate sample bounds, avoid square-root
  distance work outside threshold ranges, and use exact segment/rectangle checks
  for orthogonal collision counting. Dense benchmark after these changes: 5.6
  seconds on May 14, 2026.
- A grid graph adjacency cache was tested and not retained. In the current
  route shape, cache-key and graph materialization overhead outweighed reuse and
  regressed the dense benchmark from roughly 6.0 seconds to 7.2 seconds.
- The next retained grid-route candidate is scan-line blocker prefiltering:
  horizontal grid segments only need blockers whose padded vertical span contains
  that y value, and vertical grid segments only need blockers whose padded
  horizontal span contains that x value.
- Scan-line blocker prefiltering was retained. It keeps grid topology unchanged
  while reducing impossible segment/blocker checks. Dense benchmark after this
  change: 5.5 seconds on May 14, 2026.
- Array-indexed grid adjacency and visited flags replaced `Map`/`Set`
  bookkeeping inside Dijkstra. This keeps pathfinding behavior unchanged while
  reducing inner-loop overhead. Dense benchmark after this cleanup: 5.25 seconds
  on May 14, 2026.
- The next optimization target is reducing grid-route invocation count, not
  further tuning grid internals. The router should measure how many edges reach
  grid routing, why cheap candidates were rejected, and whether bounded cheap
  candidates can be expanded before invoking Dijkstra.
- Dense benchmark measurement showed 67 of 395 routed edges escalated to grid
  routing, but those edges caused 9,188 grid-route calls. Most cheap-candidate
  rejections were crossings, but accepting those blindly would violate the
  crossing avoidance invariant. The safer optimization is reducing grid port
  fan-out while leaving the broad cheap candidate set intact.
- Bounded grid port fan-out was retained. Cheap routing still evaluates the broad
  aligned port set, but grid routing now uses representative offsets only. This
  reduced dense benchmark grid-route calls from 9,188 to 4,324 and moved the
  benchmark to 4.2 seconds on May 14, 2026.

Remaining ratchets:

- Keep `complex-fan-out` at zero perimeter fallback routes.
- Keep `complex-fan-in` at zero perimeter fallback routes.
- Keep `complex-c4-component` at zero perimeter fallback routes.
- Keep Architext self Deployment structural routes and Data/Risks active-flow
  routes from sharing visible orthogonal route segments.
- Keep `endpointStackCost`, `doglegCost`, `monotonicBacktrackCost`,
  `labelConflictCost`, and `labelNodeConflictCost` at zero for complex fixtures
  unless the fixture is explicitly modeling an unavoidable warning.
- Keep broad manual benchmarks outside formal lifecycle checks until their
  failures are reduced to committed synthetic fixtures.

Initial complex fixtures:

- `complex-fan-out` covered: one source routes to multiple targets around intervening
  nodes.
- `complex-fan-in` covered: multiple sources converge on one target without
  sharing an unreadable endpoint stack.
- `complex-crossing-hops` covered: accepted perpendicular intersections are
  rendered with hops after route selection.
- `complex-c4-component` covered: C4-style lanes route through the same planner as
  system maps.
- `complex-too-close` covered: deliberately cramped nodes produce explicit warnings
  rather than hiding the failure behind a convoluted path.

## Dense Benchmark Baseline

Manual dense routing experiments during development exposed route/node
collisions in dense views. Those lessons are now represented by committed
synthetic fixtures. The first routing improvement made node-body collisions a
dominant selection constraint and added obstacle-aware orthogonal candidates.

Headless route checks covered non-C4, non-sequence views with both structural
relationships and flow relationships.

Initial collision baseline:

| View | Type | Relationship Set | Relationships | Route Collisions |
| --- | --- | --- | ---: | ---: |
| `system-map` | `system-map` | structural | 77 | 20 |
| `system-map` | `system-map` | flow | 65 | 24 |
| `agent-turn-flow` | `flow-explorer` | structural | 24 | 2 |
| `agent-turn-flow` | `flow-explorer` | flow | 32 | 1 |
| `dataflow-sensitive` | `dataflow` | structural | 46 | 13 |
| `dataflow-sensitive` | `dataflow` | flow | 38 | 12 |
| `deployment-local` | `deployment` | structural | 12 | 2 |
| `deployment-local` | `deployment` | flow | 13 | 3 |
| `risk-overlay` | `risk-overlay` | structural | 53 | 11 |
| `risk-overlay` | `risk-overlay` | flow | 35 | 5 |

Current benchmark:

| View | Type | Relationship Set | Relationships | Route Collisions |
| --- | --- | --- | ---: | ---: |
| `system-map` | `system-map` | structural | 77 | 0 |
| `system-map` | `system-map` | flow | 65 | 0 |
| `agent-turn-flow` | `flow-explorer` | structural | 24 | 0 |
| `agent-turn-flow` | `flow-explorer` | flow | 32 | 0 |
| `dataflow-sensitive` | `dataflow` | structural | 46 | 0 |
| `dataflow-sensitive` | `dataflow` | flow | 38 | 0 |
| `deployment-local` | `deployment` | structural | 12 | 0 |
| `deployment-local` | `deployment` | flow | 13 | 0 |
| `risk-overlay` | `risk-overlay` | structural | 53 | 0 |
| `risk-overlay` | `risk-overlay` | flow | 35 | 0 |

All routes have finite geometry. `first-party-surfaces` (`c4-container`) and
`release-gate-flow` (`sequence`) were skipped because those views still use
separate drawing logic.

The benchmark learnings are now covered by committed synthetic fixtures that
exercise both orthogonal and spline route rendering modes against the same
obstacle-aware geometry. Spline-mode collision checks use samples from the
rendered spline path, not only the pre-smoothed polyline. The next correctness
target is to bring C4 routing under the same pure routing API and then add
label-box collision checks.

## Implementation Sequence

1. Extract the current route planner into a pure module without changing visual
   behavior.
2. Add fixture tests that check determinism, finite geometry, collision
   avoidance, and fan-out uniqueness.
3. Introduce a holistic `planDiagram` pass that computes nodes, approximate
   label boxes, lanes, route corridors, and warnings before drawing SVG/HTML
   elements.
4. Add a debug overlay hidden behind `?debugRouting=1`.
   The overlay should read directly from `planDiagram` output and show route
   warnings, label warnings, and dominant named cost components. It must not
   have separate routing math.
5. Replace the current candidate-scoring approach with library-derived routing
   concepts:
   - plan all edges against fixed node rectangles before rendering
   - use explicit source and target port candidates
   - use perpendicular source and target port stubs
   - support flexible side-port placement instead of side-midpoint anchoring
   - apply monotonic path restrictions where source-to-target direction is clear
   - prefer center/direct routes first, then space-distributed alternatives
   - bound candidate search and report search-exhausted warnings
   - score named costs: node collisions, edge crossings, repeated crossings,
     bends, long corridors, shallow doglegs, label conflicts, and perimeter
     fallback
   - reserve bridge/hop rendering for accepted perpendicular intersections after
     route selection
   - handle same-side port spacing with geometry first and color/z-order second
   - return route warnings for least-bad fallbacks and too-close node
     arrangements
6. Use ELK, libavoid, yFiles, and JointJS as algorithm references, not default
   dependencies.
7. Add optional schema-supported routing hints only after automatic routing has
   measurable coverage.

## Spline Routing Track

Spline routing must not mean "draw arbitrary Bézier edges and hope they look
better." It needs the same geometry discipline as orthogonal routing: fixed
inputs, sampled paths, collision checks, label scoring, and deterministic
output.

Near-term approach:

- Plan splines with spline-specific geometry. Orthogonal route waypoints are not
  valid spline waypoints; smoothing them produces warped orthogonal output.
- Choose source/target ports and Bézier control points as first-class spline
  candidates, then sample and score those curves against node rectangles,
  labels, other routes, and boundaries.
- Spline mode should produce visible curved paths. A straight cubic command is
  not the intended spline presentation; straight-line presentation would be a
  separate future style, not the current spline option.
- Keep the route samples tied to the rendered curve, not only the pre-smoothed
  polyline, before claiming collision correctness for spline mode. This is now
  covered for the spline rendering path.
- Score curve candidates by node clearance, label clearance, bend smoothness,
  edge-edge proximity, and route length.
- Dense spline views must avoid reusing the same visible channel for unrelated
  routes. Parallel or nearly parallel curves may run near each other briefly at
  a shared source/target fan-out, but long close runs indicate a missing
  route-index penalty or insufficient candidate diversity.
- Preserve style purity: a view rendered in spline mode uses spline edges
  consistently; a view rendered in orthogonal mode uses orthogonal edges
  consistently.

Algorithm ideas to lift:

- **Bezier spline post-processing:** transform selected polyline/orthogonal
  routes into smooth cubic or quadratic segments while preserving anchors and
  obstacle clearance.
- **Tangent-visibility routing:** treat node rectangles as inflated obstacles
  and generate curve control points from visible tangent corridors.
- **Geometric control-point modeling:** make control points explicit route data
  so curves can be sampled, scored, debugged, and tested.
- **Edge bundling:** consider only for dense overview modes. Bundling can reduce
  clutter, but it can also hide individual dependency paths and should not be
  the default for workflow or C4 views.

Deferred ideas:

- Force-directed edge bundling is useful for large network visualizations, but
  it is iterative, less deterministic, and can obscure individual architecture
  relationships.
- Differential-equation-based routing is too complex for Architext's current
  needs and should not be introduced without a concrete fixture that simpler
  geometric routing cannot solve.
- Curve-based planar graph routing is aimed at general graph traversal problems,
  not the fixed-node architecture diagrams Architext currently renders.

## Debug Overlay

The debug overlay should be disabled by default and enabled with:

```text
?debugRouting=1
```

It should show:

- node rectangles
- chosen route samples
- label boxes
- selected route points and warning-colored route points
- route cost
- collision warnings

## Verification

Routing changes should run:

```sh
npm run verify
```

Before release packaging, run:

```sh
npm run release:check
```

For visual changes, update the self-hosted screenshots only after the geometry
tests pass.
