# Architext Template

This directory is the project-local Architext implementation. It renders the
JSON architecture model in `data/` as a local read-only engineering site.

## Commands

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

