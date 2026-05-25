# Validator Input Hardening

Architext validators protect repository-owned JSON. They should report bad
shape as validation output instead of crashing the caller or treating malformed
metadata as current.

## Contract

- Reference validation tolerates missing optional array fields and treats them as
  empty lists.
- Required top-level collections that are not arrays produce explicit validation
  errors.
- Schema migration planning validates semantic-version shape before declaring
  data current.
- Malformed current or target schema versions produce a pending `invalid`
  migration item rather than using lexicographic ordering.

## Verification

- Malformed node/flow/view arrays return validation messages instead of
  throwing.
- Equal malformed schema versions are not `upToDate`.
- Malformed schema versions produce `invalid` pending migration work.
