# Release Planning UI Behavior Tests

Release Planning UI tests must verify user-visible behavior through exported
presentation helpers rather than matching component source text.

## Architecture

The browser UI is TypeScript/TSX, while the repository test runner executes
plain Node tests. UI behavior that matters outside a specific DOM tree should
therefore live in small JavaScript presentation helpers that both sides can
import.

This keeps tests stable across JSX formatting, component extraction, and class
name reshuffling. Component files remain responsible for wiring inputs,
callbacks, and rendering; helper modules own deterministic presentation
decisions such as:

- whether release-plan actions are disabled;
- how release-plan API payloads omit temporary ad hoc IDs;
- which release detail must be loaded after a release-planning refresh;
- what notice appears when data changes while editors are dirty;
- which item summary text is projected into the Release Truth path.

## Verification

- Tests import the helper modules directly.
- Tests do not read `main.tsx` or `ReleasePlanning.tsx` as raw source.
- The component and main app use the same helpers the tests assert.
