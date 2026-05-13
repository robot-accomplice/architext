# LLM Architext Contract

Architext JSON files are the machine-readable architecture source of truth for
this project.

When architecture changes, update the relevant files under `data/` before
claiming the implementation is complete.

## Required Behavior

- Read existing Architext data before editing it.
- Reuse existing IDs for existing concepts.
- Create new nodes before referencing them from flows or views.
- Keep flows ordered.
- Update data classification whenever data movement changes.
- Update risks when adding external dependencies, persistence, async
  processing, sensitive data handling, trust boundary crossings, or operational
  complexity.
- Prefer source-path-backed claims.
- Mark uncertainty explicitly instead of inventing details.
- Run `npm run validate` after changing data.
- Do not claim Architext is current if validation failed or was skipped.

## Update Triggers

Update Architext when changing:

- module or service responsibilities
- public or internal APIs
- queues, topics, jobs, or workers
- data stores
- external integrations
- authentication or authorization behavior
- trust boundaries
- deployment topology
- observability paths
- sensitive data handling
- core business flows
- architecture decisions
- known architecture risks

