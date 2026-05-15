# Routing Framework Comparison

Research date: May 14, 2026.

## Context

Architext renders architecture maps from committed JSON data. The viewer is not a
general-purpose diagram editor: target repositories should own architecture
facts, not viewer implementation or hand-authored coordinates. Routing therefore
needs to be deterministic, local, testable, and compatible with package-owned
viewer assets.

The key design lesson is that routing cannot be a drawing-time afterthought.
Architext needs a holistic diagram planning pass that evaluates nodes, lanes,
labels, edge density, route corridors, and warnings before any SVG paths or HTML
cards are drawn.

Primary drivers:

- avoid non-endpoint node bodies
- keep route output stable for stable data
- support per-view orthogonal or curved rendering without mixing styles
- preserve Architext's lane/row architecture layout language
- keep target repositories data-only
- keep package installation simple
- avoid commercial or operational dependencies for core functionality

License posture: this document compares public capabilities and documented
algorithm concepts. Architext does not copy, port, bundle, wrap, or link source
code from these routing libraries. If that changes, the implementation must be
reviewed as a dependency decision and
[Third-Party Notices](../../THIRD_PARTY_NOTICES.md) must be updated before
release.

## Candidates

| Candidate | Type | License / Terms | Strengths | Weaknesses | Fit |
| --- | --- | --- | --- | --- | --- |
| ELK / elkjs | layout engine | EPL-2.0 | Mature hierarchical layout, ports, labels, compound graphs, JavaScript package | Wants to own more layout than Architext currently delegates; option surface is large | Algorithm reference |
| libavoid / Adaptagrams | routing library | LGPL, with commercial dual licensing available | Purpose-built object-avoiding orthogonal/polyline connector routing | C++ library; browser integration likely requires WASM or wrapper work; LGPL obligations are not appropriate for a casual bundled dependency | Good algorithm reference; risky dependency |
| yFiles | commercial SDK | Commercial | Best-in-class routing: orthogonal, octilinear, curved, labels, bus routing, incremental routing | Commercial licensing and SDK lock-in; unsuitable as default OSS dependency | Reference only |
| JointJS | diagram framework | MPL-2.0 for Community; JointJS+ is commercial | Built-in Manhattan and metro routers with obstacle avoidance; SVG-based | Framework replacement pressure; some advanced features are commercial JointJS+ | Possible reference, poor default fit |
| Cytoscape.js | graph visualization framework | MIT | Mature interactive visualization and layout ecosystem | Optimized for network graphs, not architecture lanes; would replace renderer model | Poor default fit |
| React Flow | React diagram framework | MIT | Excellent interaction model and built-in edge styles | Edge styles are renderers, not a serious obstacle-avoiding router | Poor routing fit |
| Graphviz / Viz.js | layout engine | EPL-2.0 for current Graphviz versions | Mature graph layout; supports spline, polyline, curved, orthogonal options | Orthogonal routing has documented port/label limitations; output less aligned with interactive Architext layout | Reference or export path |
| Sprotty + ELK | diagram framework plus layout engine | Eclipse open-source project; commonly paired with EPL-2.0 ELK | Strong model-driven diagram stack with ELK integration | Heavy framework adoption; dependency injection and SModel add complexity | Too large for current viewer |
| D2 / TALA | diagram tool and layout engine | D2 is MPL-2.0; TALA is proprietary/closed-source | TALA is architecture-diagram-oriented and orthogonal | TALA licensing and closed-source distribution are incompatible with Architext's default runtime | Reference only |

## Source Notes

- ELK is a collection of graph drawing algorithms; elkjs is its JavaScript
  cousin and is used in academic and commercial projects.
- Sprotty integrates ELK by transforming Sprotty models into ELK graph elements
  and transferring computed positions back.
- libavoid is explicitly about object-avoiding polyline and orthogonal connector
  routing for interactive diagram editors.
- yFiles EdgeRouter can route fixed-node diagrams as orthogonal, octilinear, or
  curved paths and supports incremental scenarios.
- JointJS's Manhattan router is a smart orthogonal router that inserts route
  points while avoiding obstacles; its metro router extends that to octilinear
  routing.
- Cytoscape.js supports rich graph visualization and several edge styles,
  including bezier, taxi, and round-taxi edges, but it is a broader graph
  visualization stack.
- React Flow has default, straight, step, smoothstep, and simple bezier edge
  renderers, but this is visual edge styling, not obstacle-aware route planning.
- Graphviz supports spline, polyline, curved, and orthogonal edge styles, but
  its own documentation says orthogonal routing currently does not handle ports
  or edge labels in dot.
- TALA is positioned as a general orthogonal layout engine for architecture-style
  diagrams, but D2 documents it as proprietary and closed-source.

## Curved Routing Techniques

Curved routing is not one algorithm family. The relevant techniques split into
practical diagram-rendering methods and broader visualization/research methods.

| Technique | Usefulness to Architext | Notes |
| --- | --- | --- |
| Bézier spline post-processing | High | Smooth an already accepted polyline/orthogonal route. This preserves Architext's lane layout and keeps collision checks tractable. |
| Geometric control-point modeling | High | Make curve control points explicit, sampled, scored, and debuggable. |
| Tangent-visibility graph routing | Medium | Useful for computing smooth corridors around inflated node obstacles. More complex than current grid routing, but aligned with fixed-node diagrams. |
| Force-directed edge bundling | Low by default | Useful for dense network overviews, but can hide individual architecture relationships and introduces iterative/non-obvious behavior. |
| Differential-equation-based routing | Low | Interesting research path, but too much complexity for the current product constraints. |
| Curve-based planar graph routing | Low | More relevant to general graph traversal than fixed-node architecture maps. |

The strongest near-term curved strategy is post-processing: first compute a
correct obstacle-aware route, then transform it into a smooth path and sample
the rendered curve for collision and label checks. This is materially different
from drawing arbitrary source-to-target Béziers.

## Assessment

### ELK / elkjs

ELK has the right conceptual model to study: nodes, edges, ports, labels,
compound graph structure, and layout options. It could be used in two ways:

1. full layout mode, where ELK places nodes and routes edges
2. routing oracle mode, where Architext supplies fixed node positions and asks
   ELK for edge sections

Neither mode should be the next step. Architext's lanes and rows are part of the
product language; even a routing-oracle integration would introduce a second
layout vocabulary before the current router has exhausted the obvious algorithm
improvements.

Best use: lift concepts, not code. ELK reinforces that ports, labels, compound
boundaries, and deterministic layout options should be first-class router inputs.
It should not be adopted as a dependency unless Architext later needs whole-graph
layout, not just edge routing.

### libavoid

libavoid maps very closely to the problem: route connectors around rectangular
obstacles. Its algorithm lineage is directly relevant to Architext's current
custom router. The practical problem is packaging. A C++ dependency would need a
maintained JavaScript/WASM story, browser compatibility checks, and npm package
integration. That is a lot of surface area for a CLI whose current distribution
goal is simple global install.

Best use: study the algorithm and data model. Do not adopt until a maintained
WASM package is proven.

### yFiles

yFiles is the strongest product capability benchmark. It supports exactly the
kind of route-style choice Architext is moving toward: orthogonal, octilinear,
and curved routes over fixed node positions. It also covers labels, grouped
graphs, incremental routing, bus routing, and advanced layout workflows.

It is not a suitable default dependency for Architext because Architext is an
open-source CLI package. Commercial SDK licensing would materially change usage,
distribution, and contributor expectations.

Best use: capability reference. Do not depend on it.

### JointJS

JointJS is close to Architext's current SVG model and its routers are relevant,
especially Manhattan obstacle avoidance. The drawback is architectural: JointJS
is a diagramming framework, not just a route function. Adopting it would pressure
Architext toward an editor-style object model and away from the current
data-only viewer model.

Best use: compare Manhattan router behavior and options. Avoid wholesale
adoption unless Architext intentionally becomes an editable diagram tool.

### Cytoscape.js

Cytoscape.js is excellent for graph visualization and analysis. It has useful
edge styles and a mature layout ecosystem. It is less aligned with Architext's
architecture-map semantics: lanes, workflow steps, C4 boundaries, detail panels,
and package-owned static viewer output.

Best use: reference for edge style ergonomics and performance patterns. Not a
routing dependency.

### React Flow

React Flow is a strong interactive diagram UI library, but its built-in edge
types are mostly path renderers. It does not solve the hard part Architext is
working on: deterministic obstacle-aware routing with label placement and dense
fan-out.

Best use: reference for UI controls and interaction conventions. Not a routing
dependency.

### Graphviz / Viz.js

Graphviz is mature and useful for generated graph diagrams. The mismatch is
control. Architext needs predictable integration with its own lane layout and
interactive details. Graphviz orthogonal routing also has explicit limitations
around ports and labels, which are central to the problems Architext still has.

Best use: possible export format or benchmark. Not the primary viewer router.

### Sprotty + ELK

Sprotty is credible for model-driven diagrams and integrates with ELK. It is too
large for the current step because it would replace much more than edge routing:
data model, rendering model, action dispatch, dependency injection, and layout
flow.

Best use: reference architecture if Architext ever grows into an editor.

### D2 / TALA

D2 and TALA are instructive because TALA targets architecture diagrams and
orthogonal layout. The licensing constraint is decisive for Architext's default
runtime: D2 documents TALA as proprietary and closed-source.

Best use: benchmark output and design vocabulary. Do not depend on it.

## Recommendation

Do not replace the current viewer with a graph framework.

Proceed in this order:

1. Keep the pure `routeEdges` API as Architext's routing boundary.
2. Add a whole-diagram planning boundary above the router so all geometry is
   evaluated together before rendering.
3. Add correctness features inside the current router first:
   - explicit port candidates
   - label bounding boxes and label collision scoring
   - edge-edge intersection counting
   - bridge/hop rendering for accepted perpendicular crossings
   - warnings when the best route is least-bad rather than clean
4. Lift proven router concepts from ELK, libavoid, yFiles, and JointJS without
   adding a dependency:
   - explicit source and target port candidates
   - obstacle-expanded visibility graph or grid graph
   - bend count, length, crossing, clearance, and label penalties
   - monotonic path preferences where architecture flow direction is known
   - edge grouping and lane spacing for fan-out/fan-in
   - bridge/hop rendering for accepted perpendicular intersections
5. Defer external engine adoption. Revisit only if the pure router cannot meet
   measurable benchmarks on Architext and Roboticus after those concepts are
   implemented.

## Library-Derived Rules To Adopt

The existing libraries suggest that Architext should stop treating routing as a
collection of edge-local penalties. Mature routers expose a clearer model:

- **Route against fixed nodes, but plan all relevant edges together.** yFiles
  EdgeRouter and OrthogonalEdgeRouter explicitly keep node positions fixed while
  routing edges around them. Architext's lane layout can stay owned by the
  viewer, but edge planning should see the whole diagram.
- **Use port candidates, not one anchor point.** ELK and yFiles both model ports
  as explicit edge attachment points. JointJS also exposes perpendicular routing
  behavior so links can connect cleanly to a nearby orthogonal point instead of
  blindly using an anchor.
- **Treat costs as first-class configuration.** yFiles exposes costs for
  crossings and other routing situations. Architext needs named, inspectable
  costs for node collisions, edge crossings, repeated crossings, bends, long
  corridors, shallow doglegs, label conflicts, and perimeter fallback.
- **Support monotonic restrictions.** yFiles documents monotonic path
  restrictions: route segments should generally move from source toward target
  instead of turning back. This maps directly to the bad routes we are seeing
  when an edge travels across the canvas, doubles back, or uses a shallow Z
  break.
- **Balance center-driven and space-driven search.** yFiles exposes a
  center-to-space ratio. Architext currently has an implicit, unstable version
  of this. We need an explicit bias: prefer local, direct routes first; spread
  only when direct routes collide or stack.
- **Bound search complexity.** JointJS Manhattan routing exposes grid step size
  and maximum loop count with fallback behavior. Architext needs the same kind
  of bounded search contract before adding more flexible ports.
- **Separate routing from rendering.** JointJS routers transform link vertices
  into route points; yFiles computes routes before drawing. Architext should
  compute route geometry, warnings, and labels in `planDiagram`, then render SVG
  from the planned geometry.
- **Use fallback warnings.** When the best route is only least-bad, the planner
  should report why: too-close nodes, no clean corridor, repeated crossing, label
  conflict, or search exhausted.

## Decision Bias

The current custom router is still justified because Architext has a constrained
diagram language: lanes, rows, known node rectangles, and a global style choice.
Those constraints make a small pure router viable. The risk is underestimating
label placement and edge-edge readability. That risk should be addressed with
tests and metrics before adopting a broad diagramming framework.

## Sources

- [ELK paper](https://arxiv.org/abs/2311.00533)
- [ELK project license](https://projects.eclipse.org/projects/modeling.elk)
- [Sprotty ELK integration](https://sprotty.org/docs/sprotty-elk/introduction/)
- [Adaptagrams libavoid](https://www.adaptagrams.org/documentation/libavoid.html)
- [Adaptagrams overview](https://www.adaptagrams.org/)
- [yFiles edge routing](https://docs.yworks.com/yfiles-html/dguide/automatic-layouts-main-chapter/polyline_router.html)
- [yFiles orthogonal edge router](https://docs.yfiles.com/yfiles/doc/api-svg/y/layout/router/OrthogonalEdgeRouter.html)
- [JointJS routers](https://docs.jointjs.com/4.0/api/routers/)
- [JointJS licensing](https://www.jointjs.com/license)
- [Cytoscape.js documentation](https://js.cytoscape.org/)
- [React Flow edge types](https://reactflow.dev/examples/edges/edge-types)
- [React Flow edge API](https://reactflow.dev/api-reference/types/edge)
- [Graphviz splines](https://graphviz.org/docs/attrs/splines/)
- [Graphviz license](https://graphviz.org/license/)
- [D2 TALA](https://d2lang.com/tour/tala/)
- [TALA license notes](https://terrastruct.com/tala/)
