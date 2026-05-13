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
- right-panel deep links to sections

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
