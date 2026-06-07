# Package Source And Target Data Split

Architext must not use the same path for package-owned source and
target-owned project state. Normal target repositories own
`docs/architext/data/**/*.json`, generated static output, and lifecycle
metadata. They must not own the viewer implementation, schemas, validator,
Vite project, or package-local documentation.

The Architext package repository is different from target repositories. It must
ship and test the package-owned viewer source. Keeping that source under
`docs/architext` makes `architext sync` migration dangerous because the same
path can mean either:

- copied implementation files that should be removed from a target repository
- package source files that are required to build, validate, serve, test, and
  publish Architext itself

The package-owned viewer therefore lives under `viewer/`. Target-owned
architecture data remains under `docs/architext/data`. Generated static output
for target repositories remains `docs/architext/dist` by default.

## Ownership Boundaries

- `viewer/`: package-owned viewer app, schemas, validator, Vite config, public
  assets, and package-local README.
- `docs/architext/data/`: target-owned architecture, Release Truth, rules,
  roadmap, risks, decisions, glossary, flows, views, and manifest data.
- `docs/architext/.architext.json`: target lifecycle metadata.
- `docs/architext/dist/`: generated static target output and ignored local
  artifact.

## Sync Consequence

Copied-install migration can still remove old `docs/architext/src`,
`docs/architext/schema`, `docs/architext/tools`, and related package-owned
paths from target repositories. In the Architext package repository, those
source paths no longer exist under the target data directory, so migration no
longer overlaps with package source.
