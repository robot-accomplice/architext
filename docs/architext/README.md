# Architext Template

This directory is the package-owned Architext viewer, schema, and starter data.
Target repositories should not copy or edit this implementation. They own only
their `docs/architext/data/*.json` files, lifecycle metadata, and optional
repository-level agent instructions.

## Commands

From a target project root, use the global Architext CLI:

```sh
architext serve
architext validate
architext build
architext doctor
architext prompt
```

Each command also accepts an optional target path:

```sh
architext serve /path/to/project
architext validate /path/to/project
```

If you are developing Architext itself, use the local npm scripts:

Install local dependencies:

```sh
npm install
```

Validate architecture data:

```sh
npm run validate
```

Run the local development server:

```sh
npm run dev
```

Build static assets:

```sh
npm run build
```

Preview the static build locally:

```sh
npm run preview
```

The npm scripts avoid shell-specific command chains so they work on Windows,
Linux, and macOS.

## Upgrades

Target repositories are data-only in Architext 1.0+. From the target project
root, use the package CLI:

```sh
architext sync
```

The script detects whether Architext data is absent, current, or from an older
copied-template install. It prompts before writing files and can create a git
branch before making changes. Migration preserves `docs/architext/data/*.json`
by default, removes copied implementation files, updates lifecycle metadata,
and corrects Architext sections in `AGENTS.md` and `CLAUDE.md`.

The script can also maintain the target repository `.gitignore`. Generated
local artifacts, especially `docs/architext/dist/`, should be ignored.

The CLI writes lifecycle metadata to `.architext.json`. Keep that file with the
project so automation can report CLI version, update time, managed instruction
files, copied-install migration state, and last validation state.

## Management

Useful project-root commands:

```sh
architext doctor
architext status --json
architext prompt --mode architecture-change
architext clean
architext clean --node-modules
architext explain nodes
```

`doctor` is read-only and should be the first command when an install looks
stale or broken.

## Data Entry Point

The viewer loads:

```text
data/manifest.json
```

The manifest points to the remaining JSON files. Keep paths local to this
directory. Do not load runtime assets, schemas, scripts, styles, or fonts from
remote URLs.

## Validation

`npm run validate` performs two checks:

- JSON Schema validation for each data file
- cross-reference validation for IDs shared across nodes, flows, views, risks,
  decisions, and data classifications

If validation fails, the architecture model is not trustworthy.
