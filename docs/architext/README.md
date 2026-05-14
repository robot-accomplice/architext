# Architext Template

This directory is the project-local Architext implementation. It renders the
JSON architecture model in `data/` as a local read-only engineering site.

## Commands

From the target project root, prefer the Architext CLI when available:

```sh
architext serve
architext validate
architext build
architext doctor
architext prompt
```

If the CLI is not installed globally, use the root package scripts created by
the adoption workflow:

```sh
npm run architext
npm run architext:validate
npm run architext:doctor
```

If you are working directly inside this directory, use the local npm scripts:

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

This directory is intended to be managed by the Architext adoption script from
the source repository. From the target project root, use the package CLI:

```sh
architext sync
```

During local development, direct path invocation remains supported:

```sh
node /path/to/architext/tools/architext-adopt.mjs
```

The script detects whether Architext is absent, current, or needs an upgrade.
It prompts before writing files and can create a git branch before making
changes. After writing artifacts, it runs `npm install` and `npm run validate`
inside `docs/architext` unless `--skip-install` or `--skip-validate` is passed.
Upgrade preserves `docs/architext/data/*.json` by default. Those files are the
project-owned architecture record and should not be overwritten by template
updates unless a maintainer explicitly passes `--overwrite-data`.

The script can also maintain the target repository `.gitignore`. Generated
local artifacts, especially `docs/architext/node_modules/` and
`docs/architext/dist/`, should be ignored. Architecture JSON, schemas, viewer
source, package files, tools, and public assets should be committed.

The CLI writes lifecycle metadata to `.architext-install.json`. Keep that file
with the project so automation can report installed version, update time,
managed instruction files, and last validation state.

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
