# Architext Architecture Plan

## Context

Architext is a reusable, project-local architecture viewer backed by strict JSON
data files. The JSON files serve two audiences:

- humans reading the rendered local site
- LLMs maintaining an explicit map of the project's architecture, dataflows,
  risks, decisions, and implementation touchpoints

The site must be usable without a hosted service. It will be served through a
tiny local static server so the browser can load JSON files with normal
`fetch()` behavior.

## Architectural Drivers

- **Correctness:** schema violations and unresolved references must stop render.
- **LLM maintainability:** data shape must be explicit, stable, and easy to
  update mechanically.
- **Human readability:** engineers should be able to inspect architecture,
  flows, data movement, and risks quickly.
- **Project locality:** each target project owns its Architext files under
  version control.
- **Low operational burden:** no hosted backend, database, or remote build
  service.
- **Local runtime assets:** no framework code, fonts, styles, schemas, or
  visualization libraries are loaded from remote URLs at runtime.
- **Cross-platform scripts:** setup, validation, build, and dev commands must
  work on Windows, Linux, and macOS without POSIX-only shell behavior.
- **Function over form:** the UI prioritizes dense navigation, diagram space,
  search, and selected-node detail over decorative presentation.

## Non-Goals

- Hosted SaaS documentation.
- Browser-specific `file://` loading behavior.
- Runtime CDN imports.
- In-browser editing of architecture JSON.
- Project-specific look and feel.
- Inferring architecture magically without reviewable JSON output.

## Local Serving Model

Architext requires a local server. The default development command should be:

```sh
cd docs/architext
npm install
npm run dev
```

The viewer loads data from `/data/manifest.json`, then follows the file list in
the manifest to load the remaining JSON files.

This avoids browser lock-in. A direct `file://` page with sibling JSON files is
not a sound baseline because browser security rules differ.

The viewer may use a frontend framework internally. Dependencies must be
installed locally and bundled or served from local project files. The running
site must not pull code, styles, fonts, schemas, or assets from remote URLs.

The build output must remain static so a copied project can also serve `dist/`
with a tiny local static server.

NPM scripts should call Node/Vite entrypoints directly and avoid shell command
chains, environment-variable syntax, or utilities that are not available on all
target operating systems.

## Adoption And Upgrade Workflow

Architext needs a cross-platform Node adoption script because copying the
template by hand is both error-prone and difficult to upgrade consistently.

The script should support two explicit operations:

- **Install:** copy the Architext template into a target repository at
  `docs/architext`. This includes the demo JSON data so developers can run a
  working site immediately and then ask an LLM to replace the example with the
  target project's architecture.
- **Upgrade:** refresh Architext viewer code, schemas, validation tooling,
  package files, and local documentation in a target repository that already
  has `docs/architext`.

Upgrade must preserve target-owned architecture data by default:

- do not overwrite `docs/architext/data/*.json`
- do overwrite template-owned implementation files such as `src/`, `schema/`,
  `tools/`, `public/`, `index.html`, `package.json`, and `package-lock.json`
- allow an explicit data overwrite flag only for demo resets or controlled
  migrations

The script should also be able to append the Architext agent mandate to a
target `AGENTS.md` or `CLAUDE.md` file when explicitly requested. It must avoid
duplicate appendix insertion by checking for the Architext heading before
appending.

The workflow must avoid POSIX-only shell behavior. Use Node filesystem APIs for
copying, directory creation, path handling, and file updates so the same command
works on Windows, Linux, and macOS.

## Template Placement

In a consuming project, the intended structure is:

```text
docs/
  architext/
    index.html
    package.json
    src/
    README.md
    LLM_ARCHITEXT.md
    AGENTS_APPENDIX.md
    schema/
      manifest.schema.json
      nodes.schema.json
      flows.schema.json
      views.schema.json
      data-classification.schema.json
      decisions.schema.json
      risks.schema.json
    data/
      manifest.json
      nodes.json
      flows.json
      views.json
      data-classification.json
      decisions.json
      risks.json
      glossary.json
    examples/
      claimsdesk/
        data/
          manifest.json
          nodes.json
          flows.json
          views.json
          data-classification.json
          decisions.json
          risks.json
          glossary.json
    tools/
      validate-architext.mjs
```

## Data Model

The data model is split by responsibility instead of stored as one large file.
`manifest.json` is the entrypoint.

### `manifest.json`

Defines:

- project identity
- schema version
- generated timestamp
- default view
- required data files
- generation notes
- validation expectations

### `nodes.json`

Defines architectural elements:

- actors
- clients
- applications
- services
- modules
- jobs/workers
- queues/topics
- data stores
- external services
- deployment/runtime units
- trust boundaries

Each node should support:

- stable ID
- type
- name
- summary
- responsibilities
- owner
- source paths
- runtime/deployment notes
- APIs, events, topics, or commands
- dependencies
- data handled
- trust/security notes
- observability hooks
- related flows
- related decisions
- known risks
- verification references

### `flows.json`

Defines ordered flows only. A flow is a scenario or system behavior with a
numbered sequence of steps.

Each flow should support:

- stable ID
- name
- summary
- status
- trigger
- actors
- ordered steps
- guarantees
- failure behavior
- data classes moved
- trust boundary crossings
- observability
- source paths
- verification references
- known gaps

Each step references existing node IDs. Unresolved references are validation
errors.

### `views.json`

Defines renderable views over the same model:

- system map
- flow explorer
- C4 context
- C4 container
- C4 component
- dataflow
- deployment/runtime
- risk and decision overlays

Views may provide lane/grouping/layout hints. They must not redefine the
project-specific visual language.

### `data-classification.json`

Defines project data classes and handling constraints:

- public metadata
- customer content
- PII
- secrets
- auth tokens
- financial data
- regulated data
- internal operational data

Flows that move data must reference these classifications.

### `decisions.json`

Defines architecture decisions or links to ADRs:

- accepted decisions
- rejected alternatives
- consequences
- related nodes
- related flows

### `risks.json`

Defines architecture risks:

- technical risks
- operational risks
- security risks
- data/privacy risks
- status
- mitigations
- related nodes
- related flows

## Schema Discipline

The schema must be strict:

- stable IDs are required
- references must resolve
- unknown top-level fields are rejected unless explicitly versioned
- ordered flow steps are required
- data classification is required for flow steps that move data
- node and flow types come from Architext-owned enums
- project-specific tags are allowed as data, not as UI behavior
- invalid data prevents rendering

## Viewer Layout

The viewer should use a dense three-region layout:

- collapsible left navigation
- central diagram canvas
- right selected-item detail panel

Required interactions:

- search
- filter by view/type/status/risk/data classification
- select node
- select flow
- highlight ordered path
- pan/zoom/fit diagram
- maximize diagram
- independent left-panel collapse and right-panel collapse
- collapse controls attached to the sidebars they control so either side can
  reclaim diagram space without hunting in a global toolbar
- persisted collapse state across reloads
- right-panel deep links to sections

Collapse behavior should follow the pattern used in Palm Command Center: a
small polished control lives on the controlled panel edge, the panel shrinks to
a narrow rail instead of disappearing entirely, and the expanded/collapsed
affordance is clear from the icon orientation. Architext needs this on both
sides because the diagram canvas is the primary work area.

The first demo currently falls short here: it only collapses the left panel from
the top toolbar, has no right-panel collapse, and hides the left panel entirely
instead of retaining a useful rail.

The right panel should be scrollable and sectioned:

- Summary
- Responsibilities
- Source paths
- Runtime/deployment
- APIs/events/topics
- Dependencies
- Data handled
- Security/trust boundary
- Observability
- Related flows
- Related decisions
- Known risks
- Verification/tests

## Look And Feel

Architext look and feel is product-owned, not project-owned.

Projects may provide names, descriptions, tags, and architecture data. Projects
must not provide custom CSS, arbitrary palettes, or custom rendering grammar.

The UI should be quiet, utilitarian, and optimized for engineers. Diagram space
is more important than branding.

Node cards should stay compact. The screenshot target shows dense cards with
short labels and secondary metadata; large dashboard cards waste diagram space.
Architext should prefer compact node boxes, lane headers, and scrollable/pannable
canvas behavior over large fixed cards.

Vertical space should be allocated the same way: non-diagram sections should
auto-size to their content, while the diagram canvas takes the remaining height.
Headers, filters, legends, and selected-flow step summaries are supporting
controls, not primary layout regions.

Flows must be visible as lines between boxes, not only as a textual list of
steps. A selected flow should draw directional edges between involved nodes,
with numbered step markers or labels where legible. The textual ordered step
list remains useful, but it is not a substitute for visual relationships.

Flow routing must optimize readability over geometric cleverness. The original
visual target uses compact boxes and readable highlighted paths; Architext
should preserve that. Lines should not take surprising paths, pass behind
related boxes, or hide numbered markers. Prefer simple direct or gently curved
paths through clear gutters, with step markers placed on readable line segments.
If a clean route cannot be drawn in a dense canvas, the renderer should choose a
simpler layout or require explicit layout hints.

Any side of a node box is a valid source or target for an edge. The renderer
should choose the least contentious attachment surface and path for the actual
node positions: under, over, or around objects is acceptable when it is clear;
behind a node or through an ambiguous overlap is not. Same-column relationships
should usually route through an outside gutter. Backward or cross-lane
relationships should reserve a clean corridor above or below the involved boxes
instead of crossing behind active nodes. The canvas should keep enough left and
top breathing room for these gutters so the columnar layout does not force
unreadable paths.

Two distinct edges should not share the same route unless there is no readable
alternative. Even when two edges connect the same pair of nodes, the renderer
should fan them into separate nearby lanes or corridors so each relationship can
be followed independently and its marker remains legible.

Architext should also support sequence diagrams as a separate view type. A
sequence diagram is not the same as the free-form flow map: it shows the ordered
messages in one selected flow across lifelines, with message numbers,
participants, and payload/data classifications.

## C4 And Architecture Views

Architext must include first-class C4-inspired views, not merely generic
groupings:

- **Context:** project/system in the center, actors and external systems around
  it, labeled relationships.
- **Container:** deployable/runtime units, databases, queues, browsers, workers,
  and external systems with communication labels.
- **Component:** major components inside a selected container, with dependencies
  and source paths.

Each view should be generated from the same JSON model. C4 views are projections
over nodes, flows, and relationships, not separate hand-maintained diagrams.

The first demo previously mislabeled lane-grouped views as C4 views. That is
not acceptable. C4 levels are semantic zoom levels, not alternate column
groupings:

- Context shows the system boundary and its relationships to actors and external
  systems. It should not expose internal containers.
- Container shows deployable/runtime units inside the system boundary plus
  external context. It should label communication protocols or interaction
  styles.
- Component shows major components inside one selected container. It should not
  mix unrelated runtime units from the whole system.

The schema needs enough relationship metadata to render these levels honestly:
relationship label, technology/protocol where known, source, target, and whether
the source/target is inside or outside the system boundary.

The UI should expose C4 as drilldown navigation:

1. **Context:** select the system boundary.
2. **Container:** drill into that system to see deployable/runtime units.
3. **Component:** drill into one selected container to see internal modules.

This should not be rendered like a selected ordered flow. Flow diagrams show
scenario paths. C4 diagrams show structural containment and static
relationships at a chosen abstraction level.

C4 diagrams should show structural connections, not workflows. A C4 Context,
Container, or Component diagram may show that one element uses, calls, reads
from, writes to, publishes to, or depends on another element. It should not show
the numbered step-by-step path for a selected flow. Ordered behavior belongs in
flow, dynamic, or sequence diagrams.

The UI implementation should now move from a generic "view dropdown" toward
work modes. Flows, sequence, C4, deployment, and data/risk review are different
jobs for engineers and should expose different left-panel navigation, diagram
controls, and details states.

The first dedicated C4 renderer does not need full Structurizr parity, but it
must stop behaving like an ordered flow diagram. It should show a system
boundary, actor/external context, relationship labels, and level switching
between context, container, and component projections. C4 edges are structural
relationships and should never use numbered workflow markers.

Diagram inspection is a core workflow. The viewer should expose zoom, fit,
reset, and focus-mode controls; selectable/hoverable edges; keyboard-focusable
nodes and relationships; and right-panel details that distinguish node, flow,
step, and relationship selections.

## Alignment Checkpoint

Against the original brief:

- **Locally hosted:** aligned. The viewer requires a local server and avoids
  `file://` behavior.
- **JSON-backed:** aligned. JSON files are the data source and LLM architecture
  map.
- **LLM-targeted markdown:** aligned as a first draft, but it still needs
  installer/adoption workflow details.
- **Consistent directory structure:** aligned as a first draft.
- **Architext-specific look and feel:** partially aligned. The plan says this,
  but the implementation still needs a more compact diagram-first layout.
- **Left navigation:** aligned.
- **Collapsible navigation:** partially aligned. Needs panel-edge controls,
  right-panel collapse, and persisted state.
- **Engineer-first UX:** partially aligned. Search/details exist, but visual
  flow lines and C4 views are missing.
- **Right-hand details panel:** aligned, but it also needs collapse behavior.
- **Fictitious example project:** aligned with ClaimsDesk.
- **AGENTS/CLAUDE mandate:** aligned as appendix text, but adoption scripting is
  still pending.

## Fictitious Example Project

The bundled example should be `ClaimsDesk`, a fictitious claims-processing SaaS.

It should include:

- web app
- auth provider
- claims API
- worker service
- document store
- queue
- fraud scoring external API
- audit log
- notification service
- analytics warehouse

Example flows:

- user signup
- claim submission
- document upload
- fraud review
- approval payout
- audit export
- admin role change

This example is broad enough to exercise auth, PII, files, queues, external
services, trust boundaries, deployment views, risks, and data classification.

## Open Questions

- Should the validator be pure browser JavaScript, Node-based, or both?
- Should diagram layout be hand-hinted in JSON or computed deterministically?
- Should future source-code extraction be plugin-based by language/ecosystem?
- Should schema version migrations be supported from the first release?
