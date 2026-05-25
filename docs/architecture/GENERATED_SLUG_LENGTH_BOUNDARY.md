# Generated Slug Length Boundary

Architext derives starter project identifiers from the target directory name.
Those identifiers become prefixes for generated node and flow IDs, so they
must stay readable and filesystem/tool friendly even when the directory name is
long.

## Contract

- Generated slugs are lowercase kebab-case.
- Empty slugs fall back to `target-project`.
- Generated slugs are capped at 64 characters.
- Truncation removes trailing separators.

This boundary applies to generated starter data. Explicit user-provided IDs are
validated by schema and are not rewritten by this helper.

## Verification

- Syncing a target with a very long directory name writes a manifest project ID
  of at most 64 characters.
