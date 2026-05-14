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

Run the Architext validator after edits:

```sh
architext validate
```

If the CLI is not available, use:

```sh
cd docs/architext && npm run validate
```

Use `architext doctor` to inspect installation health and `architext prompt` to
print the current LLM build-out or maintenance instructions. Do not claim the
architecture documentation is current if validation fails or was skipped.
```
