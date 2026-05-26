# Validator JSON Error Boundary

`docs/architext/tools/validate-architext.mjs` is a user-facing validation
entrypoint. Malformed JSON is validation input, not an internal crash.

## Contract

- Malformed architecture JSON exits non-zero.
- The error is reported as an Architext validation failure.
- The message identifies the offending file.
- Parser stacks and raw Node exception frames are not printed for malformed
  user data.

This keeps `architext validate`, `npm run validate`, and recovery workflows
actionable when a JSON edit is syntactically broken.

## Verification

- A corrupted `nodes.json` fixture reports `Invalid JSON in .../nodes.json`.
- The validator output does not include `SyntaxError` stack frames or `at
  JSON.parse`.
