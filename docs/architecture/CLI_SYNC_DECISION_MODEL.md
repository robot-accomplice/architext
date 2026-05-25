# CLI Sync Decision Model

`architext-cli.mjs` still owns the sync use case wiring, but deterministic sync
decisions must not stay buried in the 1,000+ line CLI adapter.

## Architecture

Sync has two kinds of logic:

- adapter work: prompts, filesystem writes, git commands, validation, locks, and
  terminal output;
- decision work: operation classification, whether a write is needed, whether
  validation should run, persisted choice shaping, and metadata patch shaping.

The decision work is pure and belongs in a small CLI sync model module with
direct tests. The adapter imports that model and remains responsible for
executing the resulting decisions.

This is the first decomposition slice for the CLI god file. It intentionally
does not move prompts, write execution, or subprocess flows yet.

## Verification

- Install, migrate, and no-op sync states derive stable operation labels.
- Write decisions include selected doctor repairs, force, instruction files,
  `.gitignore`, and root script management.
- Metadata patches preserve the existing sync metadata contract.
- Tests import the sync model directly instead of driving the whole CLI process.
