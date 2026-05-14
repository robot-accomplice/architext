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
architext validate [path]
```

Use the local viewer for review:

```sh
architext serve [path]
```

The optional path defaults to the current directory. Target repositories should
not vendor or edit Architext viewer, schema, tool, package, or Vite files.
Those are owned by the globally installed `architext` package. Edit project
architecture data under `docs/architext/data/*.json`; use `architext sync
[path]` to install or migrate lifecycle metadata and instructions.

Use `architext doctor [path]` to inspect installation health and `architext
prompt [path]` to print the current LLM build-out or maintenance instructions.
Do not claim the architecture documentation is current if validation fails or
was skipped.
```
