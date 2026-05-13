# Architext

Architext is a local, project-owned architecture and dataflow site generated
from strict JSON files.

It is meant for teams using LLMs to build and maintain software. The rendered
site gives humans a navigable view of the system. The JSON gives future LLMs a
stable architecture map they can read before changing code.

Architext is not a hosted documentation platform. It is a template copied into a
project repository, versioned alongside the code, and served locally.

## Why This Exists

Architecture documentation usually fails in one of two ways:

- it is prose written for humans and too vague for LLMs to use reliably
- it is generated from code and misses intent, risks, decisions, and data
  movement

Architext takes a different position: the machine-readable architecture model is
the source of truth, and the human site is a projection of that model.

The JSON is intentionally not optimized for hand editing. LLMs are expected to
maintain it as architecture changes. Humans review the rendered site and the
JSON diffs.

## What Architext Tracks

Architext is intended to describe:

- systems, services, modules, jobs, workers, queues, stores, and external
  services
- ordered application and infrastructure flows
- data movement and data classification
- trust boundaries and security controls
- runtime and deployment topology
- ownership and source-code locations
- observability paths
- architectural decisions
- known risks and gaps
- verification commands or tests tied to architectural claims

The goal is not just to draw diagrams. The goal is to preserve enough structured
context that an LLM working later can understand what exists, where it lives,
why it exists, and what must stay true.

## Design Principles

- **Local first:** every project owns its own Architext files.
- **Read-only viewer:** editing happens through JSON changes, not the browser.
- **Strict schema:** invalid data should prevent rendering.
- **LLM-maintained:** JSON is structured for machine upkeep, not casual manual
  authoring.
- **Human-readable output:** engineers should be able to inspect flows and
  components quickly.
- **Ordered flows:** flows are explicit step-by-step paths, not loose dependency
  graphs.
- **Project-neutral look and feel:** projects provide data, not custom UI
  behavior.
- **No hosted dependency:** the site runs from a local dev server or static
  build.
- **No runtime CDN:** scripts, styles, fonts, schemas, and assets must be local
  to the repository or bundled into the build.

## Planned Experience

The viewer will use a dense engineering layout:

- collapsible navigation on the left
- large diagram canvas in the center
- selected-node and selected-step details on the right
- search and filters
- pan, zoom, fit, and maximize controls
- highlighted ordered paths through flows
- scrollable detail sections for architecture, security, data, risks, and tests

The UI should be functional before it is pretty. Diagram space, legibility, and
fast inspection matter more than branding.

## Install Or Upgrade In A Project

From a target project repository, invoke the Architext adoption script by path:

```sh
cd /path/to/your-project
node /path/to/architext/tools/architext-adopt.mjs
```

The default `sync` behavior detects the current state:

- if `docs/architext` is absent, it installs Architext
- if `docs/architext/package.json` has an older/different template version, it
  upgrades Architext
- if the installed template version is current, it leaves template files alone
  unless `--force` is passed

The script prompts before writing changes. In a git repository, it also asks
whether to use the current branch or create a new branch first.

After writing Architext artifacts, the script runs the project-local setup and
validation steps for you:

```sh
npm install
npm run validate
```

Those commands run inside `docs/architext`. Use `--skip-install` or
`--skip-validate` when you explicitly need to defer those steps.

Install explicitly:

```sh
node /path/to/architext/tools/architext-adopt.mjs install
```

Upgrade explicitly:

```sh
node /path/to/architext/tools/architext-adopt.mjs upgrade
```

Run non-interactively:

```sh
node /path/to/architext/tools/architext-adopt.mjs sync --yes --branch current --append-agents
```

Useful options:

- `--target <repo>` operates on a repository other than the current directory.
- `--dry-run` shows intended changes without writing files.
- `--branch new --branch-name <name>` creates a branch before writing.
- `--branch current` writes to the current branch.
- `--append-agents` creates or appends both `AGENTS.md` and `CLAUDE.md` with the
  Architext instructions.
- `--no-agents` skips `AGENTS.md` and `CLAUDE.md` prompts.
- `--skip-install` skips dependency installation after writing artifacts.
- `--skip-validate` skips architecture JSON validation after writing artifacts.
- `--force` refreshes template-owned files even when the installed version is
  current.

Upgrade preserves `docs/architext/data/*.json` by default because those files
belong to the target project. It refreshes the viewer, schemas, validation
tooling, package files, and Architext docs. Use `--overwrite-data` only when
intentionally resetting the target architecture data to the template demo.

## Local Usage

From a project that has adopted Architext:

```sh
cd docs/architext
npm run dev
```

Then open:

```text
http://localhost:4317/
```

Architext requires a local server instead of direct `file://` loading. That
avoids browser-specific restrictions around fetching local JSON files.

The running site must not fetch framework code, stylesheets, fonts, or assets
from remote URLs.

For static usage after a build:

```sh
npm run build
cd dist
python3 -m http.server 4317
```

Project scripts should remain cross-platform. Avoid shell-specific command
chains in npm scripts so the same commands work on Windows, Linux, and macOS.

## LLM JSON Build-Out Prompt

After installing Architext into a target repository, give the project LLM a
direct instruction like this:

```text
You are working in this repository. Build out Architext for this project.

First read:
- AGENTS.md and/or CLAUDE.md if present
- docs/architext/LLM_ARCHITEXT.md
- docs/architext/README.md
- docs/architext/schema/*.schema.json
- docs/architext/data/*.json

Then inspect the codebase and replace the ClaimsDesk demo data with this
project's real architecture data. Update only docs/architext/data/*.json unless
the schema or Architext template itself is clearly wrong.

Required output:
- nodes.json: real actors, systems, services, clients, modules, workers,
  queues/topics, data stores, external services, deployment units, and trust
  boundaries
- flows.json: ordered user/system/data flows with real source and target node
  IDs, data classes, guarantees, failure behavior, observability, and
  verification references
- views.json: system map, dataflow, deployment, sequence, and C4 context /
  container / component projections using existing node IDs
- data-classification.json: data classes actually handled by the project
- decisions.json: accepted architecture decisions or links to existing ADRs
- risks.json: real architecture, security, privacy, operational, and data risks
- glossary.json: project terms that future LLMs need to understand
- manifest.json: project identity, default view, and file references

Rules:
- Reuse stable IDs for existing concepts.
- Create nodes before referencing them from flows or views.
- Keep flows ordered.
- Do not invent certainty. Mark unknowns and known gaps explicitly.
- Prefer source-path-backed claims.
- Do not edit application code for this task.
- Run `cd docs/architext && npm run validate` before claiming completion.
- If validation fails, fix the JSON and rerun it.

When finished, summarize what files changed, what architecture areas are well
covered, what remains uncertain, and the validation result.
```

## Expected Project Structure

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
    tools/
      validate-architext.mjs
```

The exact files may evolve, but the split is intentional: nodes, flows, views,
data classification, decisions, and risks are separate concerns.

## Data Model Overview

`manifest.json` is the entrypoint. It identifies the project, schema version,
default view, and data files to load.

`nodes.json` describes architectural elements such as services, modules,
clients, actors, data stores, queues, workers, external services, and trust
boundaries.

`flows.json` describes ordered flows. Each step references known nodes and
documents what moves, what is validated, what can fail, and what proves the
behavior.

`views.json` describes how the same model is projected into system maps, C4
views, dataflow diagrams, deployment views, and risk overlays.

`data-classification.json` defines the data categories used by flows and nodes.

`decisions.json` and `risks.json` connect architecture facts to the reasoning
and tradeoffs behind them.

## LLM Workflow

An LLM working in a project that uses Architext should:

1. Read the existing Architext data before changing architecture.
2. Update the relevant JSON when architecture changes.
3. Reuse existing IDs for existing concepts.
4. Add new nodes before referencing them in flows.
5. Keep flows ordered.
6. Update data classification when data movement changes.
7. Update risks when adding persistence, external services, trust boundaries,
   sensitive data, async processing, or operational complexity.
8. Run validation before claiming the task is complete.

Broken architecture JSON is worse than missing JSON because it gives future
humans and LLMs false confidence.

## Example Project

Architext will include a fictitious example called `ClaimsDesk`: a
claims-processing SaaS with a web app, claims API, document store, queue,
worker, fraud scoring integration, audit log, notification service, and
analytics warehouse.

The example exists to show what a finished Architext site should feel like
before a real project adopts the template.

## Repository Status

This repository is in the planning stage. Architecture and documentation are
being defined before implementation.

Current planning documents:

- [Architecture Plan](docs/architecture/ARCHITECTURE_PLAN.md)
- [LLM Architext Contract](docs/architecture/LLM_ARCHITEXT.md)
- [Agent Instructions Appendix](docs/architecture/AGENTS_APPENDIX.md)

## Attribution

Architext was inspired by [Dave J's x.com post about interactive architecture
and flow visualization](https://x.com/davej/status/2053867258653339746?s=46&t=e_qP9a_xUWuOJ6eKxFpaAQ).

## License

MIT. See [LICENSE](LICENSE).
