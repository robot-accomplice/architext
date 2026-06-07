# Validator JSON Error Boundary

`viewer/tools/validate-architext.mjs` is a user-facing validation
entrypoint. Malformed JSON is validation input, not an internal crash.

## Contract

- Malformed architecture JSON exits non-zero.
- The error is reported as an Architext validation failure.
- The message identifies the offending file.
- Parser stacks and raw Node exception frames are not printed for malformed
  user data.

This keeps `architext validate`, `npm run validate`, and recovery workflows
actionable when a JSON edit is syntactically broken.

The CLI validator must also enforce the same release-reference contract as the
viewer loader. Roadmap items may target a release only after that release exists
in `docs/architext/data/releases/index.json`. Otherwise a command-line
validation pass can leave the browser in recovery mode even though the local
validation command appeared green.

## Verification

- A corrupted `nodes.json` fixture reports `Invalid JSON in .../nodes.json`.
- The validator output does not include `SyntaxError` stack frames or `at
  JSON.parse`.
- A roadmap item with an unknown `targetReleaseId` exits non-zero and reports
  the unknown release id.
