# Architext Agent Instructions Appendix

Add this section to a target project's `AGENTS.md` or `CLAUDE.md` when adopting
Architext.

```markdown
## Architext Architecture Documentation

This project uses `docs/architext/data/*.json` as the machine-readable
architecture source of truth.

When changing architecture, data flow, persistence, external integrations, trust
boundaries, deployment topology, observability paths, or major module
responsibilities, update the relevant Architext JSON files before completing the
task.

Run the Architext validator after edits. Do not claim the architecture
documentation is current if validation fails or was skipped.
```

